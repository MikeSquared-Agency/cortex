use super::cache::BriefingCache;
use super::renderer::{BriefingRenderer, CompactRenderer, MarkdownRenderer};
use super::{Briefing, BriefingScope, BriefingSection};
use crate::error::Result;
use crate::graph::{GraphEngine, TraversalDirection, TraversalRequest};
use crate::storage::{NodeFilter, Storage};
use crate::types::{Node, NodeId, NodeKind, Relation};
use crate::trust::{TrustConfig, TrustEngine};
use crate::vector::{EmbeddingService, HybridQuery, HybridSearch, VectorIndex};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn pluralise(word: &str) -> String {
    if word.ends_with('y')
        && !word.ends_with("ey")
        && !word.ends_with("ay")
        && !word.ends_with("oy")
    {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with("sh")
        || word.ends_with("ch")
    {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    }
}

fn kind_to_section_title(kind: &str) -> String {
    let title_cased = kind
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    pluralise(&title_cased)
}

/// Maps node kinds to briefing roles. Each role determines how nodes of that
/// kind are presented in the briefing. Kinds not mapped to any role are handled
/// by auto-discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BriefingRoleConfig {
    /// Kinds used to locate the agent's identity node. Default: `["agent"]`
    pub identity: Vec<String>,
    /// Kinds for persistent standing context (linked to agent via applies_to).
    /// Default: `["preference"]`
    pub persistent: Vec<String>,
    /// Kinds for goal/task tracking via graph traversal. Default: `["goal"]`
    pub trackable: Vec<String>,
    /// Kinds for time-windowed recent items. Default: `["event"]`
    pub temporal: Vec<String>,
    /// Kinds ranked by access count + importance. Default: `["pattern"]`
    pub reviewable: Vec<String>,
    /// Kinds where newer nodes replace older ones. Default: `["fact", "decision"]`
    pub superseding: Vec<String>,
}

impl Default for BriefingRoleConfig {
    fn default() -> Self {
        Self {
            identity: vec!["agent".into()],
            persistent: vec!["preference".into()],
            trackable: vec!["goal".into()],
            temporal: vec!["event".into()],
            reviewable: vec!["pattern".into()],
            superseding: vec!["fact".into(), "decision".into()],
        }
    }
}

impl BriefingRoleConfig {
    /// All kinds mapped to a role. These are excluded from auto-discovery.
    pub fn mapped_kinds(&self) -> HashSet<String> {
        let mut all = HashSet::new();
        all.extend(self.identity.iter().cloned());
        all.extend(self.persistent.iter().cloned());
        all.extend(self.trackable.iter().cloned());
        all.extend(self.temporal.iter().cloned());
        all.extend(self.reviewable.iter().cloned());
        all.extend(self.superseding.iter().cloned());
        all
    }
}

/// Configuration for the briefing engine
pub struct BriefingConfig {
    pub max_items_per_section: usize,
    pub max_total_items: usize,
    pub max_chars: usize,
    pub recent_window: Duration,
    pub cache_ttl: Duration,
    pub include_contradictions: bool,
    pub min_importance: f32,
    pub min_weight: f32,
    pub exclude_kinds: Vec<String>,
    /// Weight given to importance in combined ranking (remainder goes to trust).
    /// Default 0.6 — set to 1.0 to disable trust-based ranking.
    pub importance_weight: f32,
    /// Trust scoring configuration. None = trust scoring disabled in briefing.
    pub trust: Option<TrustConfig>,
    /// Role-based kind mapping for briefing sections
    pub roles: BriefingRoleConfig,
    /// Optional per-role section title overrides (role name → custom title)
    pub titles: HashMap<String, String>,
}

impl Default for BriefingConfig {
    fn default() -> Self {
        Self {
            max_items_per_section: 10,
            max_total_items: 50,
            max_chars: 8000,
            recent_window: Duration::from_secs(48 * 3600),
            cache_ttl: Duration::from_secs(300),
            include_contradictions: true,
            min_importance: 0.3,
            min_weight: 0.2,
            exclude_kinds: vec![],
            importance_weight: 0.6,
            trust: None,
            roles: BriefingRoleConfig::default(),
            titles: HashMap::new(),
        }
    }
}

/// Graph-aware context briefing synthesiser
pub struct BriefingEngine<S, E, V, G>
where
    S: Storage,
    E: EmbeddingService + Clone,
    V: VectorIndex + Clone,
    G: GraphEngine + Clone,
{
    storage: Arc<S>,
    graph: G,
    vectors: V,
    embeddings: E,
    cache: Mutex<BriefingCache>,
    graph_version: Arc<AtomicU64>,
    config: BriefingConfig,
}

impl<S, E, V, G> BriefingEngine<S, E, V, G>
where
    S: Storage,
    E: EmbeddingService + Clone,
    V: VectorIndex + Clone,
    G: GraphEngine + Clone,
{
    pub fn new(
        storage: Arc<S>,
        graph: G,
        vectors: V,
        embeddings: E,
        graph_version: Arc<AtomicU64>,
        config: BriefingConfig,
    ) -> Self {
        let cache = Mutex::new(BriefingCache::new(config.cache_ttl));
        Self {
            storage,
            graph,
            vectors,
            embeddings,
            cache,
            graph_version,
            config,
        }
    }

    /// Generate a tailored briefing for the given agent.
    /// Returns a cached result if the graph version has not changed.
    pub fn generate(&self, agent_id: &str) -> Result<Briefing> {
        let current_version = self.graph_version.load(Ordering::Relaxed);

        // Serve from cache if version unchanged
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(agent_id, current_version) {
                let mut result = cached.clone();
                result.cached = true;
                return Ok(result);
            }
        }

        let agent_node_id = self.find_agent_node(agent_id)?;

        let mut sections: Vec<BriefingSection> = Vec::new();
        let mut seen_ids: HashSet<NodeId> = HashSet::new();

        // 1. Identity & Preferences
        let identity = self.generate_identity(agent_id, agent_node_id)?;
        if !identity.nodes.is_empty() {
            for n in &identity.nodes {
                seen_ids.insert(n.id);
            }
            sections.push(identity);
        }

        // Graph-based sections: use agent node traversal if available,
        // otherwise fall back to global queries by node kind.
        if let Some(aid) = agent_node_id {
            // 2. Reviewable (patterns etc via graph traversal)
            let reviewable = self.generate_reviewable(aid, &seen_ids)?;
            if !reviewable.nodes.is_empty() {
                for n in &reviewable.nodes {
                    seen_ids.insert(n.id);
                }
                sections.push(reviewable);
            }

            // 3. Trackable (goals etc via graph traversal)
            let trackable = self.generate_trackable(aid, &seen_ids)?;
            if !trackable.nodes.is_empty() {
                for n in &trackable.nodes {
                    seen_ids.insert(n.id);
                }
                sections.push(trackable);
            }

            // 4. Unresolved Contradictions
            if self.config.include_contradictions {
                let unresolved = self.generate_unresolved(aid, &seen_ids)?;
                if !unresolved.nodes.is_empty() {
                    for n in &unresolved.nodes {
                        seen_ids.insert(n.id);
                    }
                    sections.push(unresolved);
                }
            }
        } else {
            // No agent node — fall back to global queries for each role's kinds
            for kind in &self.config.roles.reviewable {
                let title = kind_to_section_title(kind);
                let section = self.generate_global_by_kind(kind, &title, &seen_ids)?;
                if !section.nodes.is_empty() {
                    for n in &section.nodes {
                        seen_ids.insert(n.id);
                    }
                    sections.push(section);
                }
            }

            for kind in &self.config.roles.trackable {
                let title = kind_to_section_title(kind);
                let section = self.generate_global_by_kind(kind, &title, &seen_ids)?;
                if !section.nodes.is_empty() {
                    for n in &section.nodes {
                        seen_ids.insert(n.id);
                    }
                    sections.push(section);
                }
            }

            for kind in &self.config.roles.superseding {
                let title = kind_to_section_title(kind);
                let section = self.generate_global_by_kind(kind, &title, &seen_ids)?;
                if !section.nodes.is_empty() {
                    for n in &section.nodes {
                        seen_ids.insert(n.id);
                    }
                    sections.push(section);
                }
            }
        }

        // 5. Temporal (recent events — before auto-discovery so temporal kinds are excluded)
        let temporal = self.generate_temporal(agent_id, &seen_ids)?;
        if !temporal.nodes.is_empty() {
            for n in &temporal.nodes {
                seen_ids.insert(n.id);
            }
            sections.push(temporal);
        }

        // 6. Auto-discovered sections (Phase 2 — novel kinds not mapped to any role)
        let auto_sections = self.generate_auto_discovered_sections(&seen_ids)?;
        for section in auto_sections {
            for n in &section.nodes {
                seen_ids.insert(n.id);
            }
            sections.push(section);
        }

        // 7. Active Context (Phase 3 — catch-all for anything not in a structured section)
        let active = self.generate_active_context(agent_id, agent_node_id, &seen_ids)?;
        if !active.nodes.is_empty() {
            for n in &active.nodes {
                seen_ids.insert(n.id);
            }
            sections.push(active);
        }

        // Enforce max_total_items across all sections
        let mut total = 0usize;
        for section in &mut sections {
            let remaining = self.config.max_total_items.saturating_sub(total);
            section.nodes.truncate(remaining);
            total += section.nodes.len();
        }
        sections.retain(|s| !s.nodes.is_empty());

        let nodes_consulted = sections.iter().map(|s| s.nodes.len()).sum();

        let briefing = Briefing {
            agent_id: agent_id.to_string(),
            generated_at: Utc::now(),
            nodes_consulted,
            sections,
            cached: false,
        };

        // Re-read the version *after* generation so the cache entry is stored
        // under the version that was current at store time.  If writes occurred
        // during generation the older `current_version` would never match a
        // future cache lookup (the version has already advanced), wasting the
        // work.  Using the post-generation version ensures the next caller at
        // that version gets a cache hit.
        let store_version = self.graph_version.load(Ordering::Relaxed);

        // Store in cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.put(agent_id, briefing.clone(), store_version);
        }

        // Update access counts (best-effort — failure must not block the caller)
        let _ = self.on_briefing_served(&briefing);

        Ok(briefing)
    }

    /// Generate a briefing with an explicit scope.
    ///
    /// - `Agent` — identical to `generate(agent_id)`.
    /// - `Shared` — agent's briefing plus a cross-agent context section.
    /// - `Unified(agents)` — multi-agent briefing for orchestrators.
    pub fn generate_with_scope(
        &self,
        agent_id: &str,
        scope: BriefingScope,
    ) -> Result<Briefing> {
        match scope {
            BriefingScope::Agent => self.generate(agent_id),
            BriefingScope::Shared => self.generate_shared(agent_id),
            BriefingScope::Unified(ref agent_ids) => self.generate_unified(agent_ids),
        }
    }

    /// Render a briefing to a string. compact=true gives ~4x higher density.
    pub fn render(&self, briefing: &Briefing, compact: bool) -> String {
        if compact {
            CompactRenderer {
                max_chars: self.config.max_chars,
            }
            .render(briefing)
        } else {
            MarkdownRenderer {
                max_chars: self.config.max_chars,
            }
            .render(briefing)
        }
    }

    /// Increment access_count for every node that appeared in the briefing.
    /// Uses batch write to avoid N individual storage transactions.
    pub fn on_briefing_served(&self, briefing: &Briefing) -> Result<()> {
        // Re-fetch each node so we write the freshest version, then batch-save.
        let mut updated: Vec<Node> = Vec::new();
        for section in &briefing.sections {
            for node in &section.nodes {
                if let Ok(Some(mut n)) = self.storage.get_node(node.id) {
                    n.record_access();
                    updated.push(n);
                }
            }
        }
        if !updated.is_empty() {
            let _ = self.storage.put_nodes_batch(&updated);
        }
        Ok(())
    }

    // --- Helpers ---

    /// Filter nodes below `min_importance` and sort by combined importance + trust,
    /// falling back to access_count. Applied uniformly across all section generators.
    fn rank(&self, mut nodes: Vec<Node>) -> Vec<Node> {
        let now = Utc::now();
        nodes.retain(|n| {
            n.importance >= self.config.min_importance
                && n.valid_until.is_none_or(|until| until > now)
        });

        // If trust scoring is configured, compute trust and blend with importance.
        if let Some(ref trust_config) = self.config.trust {
            let trust_engine = TrustEngine::new(self.storage.clone(), trust_config.clone());
            if let Ok(scores) = trust_engine.score_batch(&nodes) {
                let trust_map: HashMap<crate::types::NodeId, f32> = nodes
                    .iter()
                    .zip(scores.iter())
                    .map(|(n, s)| (n.id, s.total))
                    .collect();

                let iw = self.config.importance_weight;
                let tw = 1.0 - iw;

                nodes.sort_by(|a, b| {
                    let a_trust = trust_map.get(&a.id).copied().unwrap_or(0.0);
                    let b_trust = trust_map.get(&b.id).copied().unwrap_or(0.0);
                    let a_combined = a.importance * iw + a_trust * tw;
                    let b_combined = b.importance * iw + b_trust * tw;
                    b_combined
                        .partial_cmp(&a_combined)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.access_count.cmp(&a.access_count))
                });
                return nodes;
            }
        }

        // Fallback: pure importance ranking.
        nodes.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.access_count.cmp(&a.access_count))
        });
        nodes
    }

    /// Derive a section title from the role name and its mapped kinds.
    /// Custom titles from config take precedence.
    fn role_section_title(&self, role: &str, kinds: &[String]) -> String {
        if let Some(custom) = self.config.titles.get(role) {
            return custom.clone();
        }
        match role {
            "identity" => "Identity & Preferences".to_string(),
            "temporal" => {
                if kinds.len() == 1 {
                    format!("Recent {}", kind_to_section_title(&kinds[0]))
                } else {
                    "Recent Activity".to_string()
                }
            }
            _ => {
                if kinds.len() == 1 {
                    kind_to_section_title(&kinds[0])
                } else {
                    kinds
                        .iter()
                        .map(|k| kind_to_section_title(k))
                        .collect::<Vec<_>>()
                        .join(" & ")
                }
            }
        }
    }

    // --- Private section generators ---

    fn find_agent_node(&self, agent_id: &str) -> Result<Option<NodeId>> {
        // Build identity kind list from role config, plus "entity" for backward compat
        let mut agent_kinds: Vec<NodeKind> = self
            .config
            .roles
            .identity
            .iter()
            .filter_map(|k| NodeKind::new(k).ok())
            .collect();
        // Always include "entity" for backward compat with entity_type: "agent" nodes
        if let Ok(entity_kind) = NodeKind::new("entity") {
            if !agent_kinds.iter().any(|k| k.as_str() == "entity") {
                agent_kinds.push(entity_kind);
            }
        }

        // Primary: Agent/entity node whose source_agent matches
        let nodes = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(agent_kinds.clone())
                .with_source_agent(agent_id.to_string())
                .with_limit(10),
        )?;

        if let Some(n) = nodes.iter().find(|n| Self::is_agent_entity(n)) {
            return Ok(Some(n.id));
        }

        // Fallback: search by tag (agents should be tagged with their ID)
        let by_tag = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(agent_kinds.clone())
                .with_tags(vec![agent_id.to_lowercase()])
                .with_limit(10),
        )?;
        if let Some(n) = by_tag.iter().find(|n| Self::is_agent_entity(n)) {
            return Ok(Some(n.id));
        }

        // Last resort: scan agent/entity nodes for title/source match
        let all_agents = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(agent_kinds)
                .with_limit(50),
        )?;

        for node in &all_agents {
            if !Self::is_agent_entity(node) {
                continue;
            }
            if node
                .data
                .title
                .to_lowercase()
                .contains(&agent_id.to_lowercase())
                || node.source.agent == agent_id
            {
                return Ok(Some(node.id));
            }
        }

        Ok(None)
    }

    /// Check if a node is an agent node — either old-style `kind: "agent"` or
    /// new-style `kind: "entity"` with tag `entity-type-agent`.
    fn is_agent_entity(node: &Node) -> bool {
        if node.kind.as_str() == "agent" {
            return true;
        }
        if node.kind.as_str() == "entity" {
            return node
                .data
                .tags
                .iter()
                .any(|t| t == "entity-type-agent");
        }
        false
    }

    fn generate_identity(
        &self,
        agent_id: &str,
        agent_node_id: Option<NodeId>,
    ) -> Result<BriefingSection> {
        let title = self.role_section_title("identity", &self.config.roles.identity);
        let mut nodes: Vec<Node> = Vec::new();

        // Standing context kinds: persistent + superseding roles
        let neighbor_kinds: HashSet<&str> = self
            .config
            .roles
            .persistent
            .iter()
            .chain(self.config.roles.superseding.iter())
            .map(|s| s.as_str())
            .collect();

        if let Some(aid) = agent_node_id {
            // Include the Agent node itself (always, regardless of importance)
            if let Ok(Some(agent_node)) = self.storage.get_node(aid) {
                nodes.push(agent_node);
            }

            // Standing context connected via AppliesTo (either direction)
            let neighbors = self.graph.neighbors(
                aid,
                TraversalDirection::Both,
                Some(vec![Relation::new("applies_to").unwrap()]),
            )?;

            let pref_nodes: Vec<Node> = neighbors
                .into_iter()
                .filter_map(|(node, _edge)| {
                    if neighbor_kinds.contains(node.kind.as_str()) {
                        Some(node)
                    } else {
                        None
                    }
                })
                .collect();

            // Rank and append (keeping the agent node at the front)
            let mut ranked = self.rank(pref_nodes);
            ranked.truncate(self.config.max_items_per_section.saturating_sub(1));
            nodes.extend(ranked);
        } else {
            // Graceful degradation: no graph node, scan storage
            let fallback_kinds: Vec<NodeKind> = self
                .config
                .roles
                .identity
                .iter()
                .chain(self.config.roles.persistent.iter())
                .filter_map(|k| NodeKind::new(k).ok())
                .collect();

            let fallback = self.storage.list_nodes(
                NodeFilter::new()
                    .with_source_agent(agent_id.to_string())
                    .with_kinds(fallback_kinds)
                    .with_min_importance(self.config.min_importance)
                    .with_limit(self.config.max_items_per_section * 2),
            )?;
            nodes.extend(self.rank(fallback));
        }

        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection { title, nodes })
    }

    fn generate_active_context(
        &self,
        agent_id: &str,
        agent_node_id: Option<NodeId>,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        // Try agent-specific first, then fall back to global
        let mut recent = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(agent_id.to_string())
                .created_after(cutoff)
                .with_limit(self.config.max_items_per_section * 3),
        )?;

        // Fallback: if agent has no recent nodes, pull from the entire graph
        if recent.is_empty() {
            recent = self.storage.list_nodes(
                NodeFilter::new()
                    .created_after(cutoff)
                    .with_min_importance(self.config.min_importance)
                    .with_limit(self.config.max_items_per_section * 3),
            )?;
        }

        // Last resort: if nothing recent, pull highest-importance nodes globally
        if recent.is_empty() {
            recent = self.storage.list_nodes(
                NodeFilter::new()
                    .with_min_importance(self.config.min_importance)
                    .with_limit(self.config.max_items_per_section * 3),
            )?;
        }

        if recent.is_empty() {
            return Ok(BriefingSection {
                title: "Active Context".to_string(),
                nodes: vec![],
            });
        }

        // Build anchor list
        let mut anchors: Vec<NodeId> = recent.iter().map(|n| n.id).collect();
        if let Some(aid) = agent_node_id {
            anchors.push(aid);
        }

        // Use titles of the most-important recent nodes as the semantic query,
        // so the vector component searches for genuinely relevant content.
        let query_text: String = {
            let mut by_importance = recent.clone();
            by_importance.sort_by(|a, b| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            by_importance
                .iter()
                .take(3)
                .map(|n| n.data.title.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        };

        // Attempt hybrid search; fall back to raw recent list if it returns nothing
        let hybrid = HybridSearch::new(
            self.storage.clone(),
            self.embeddings.clone(),
            self.vectors.clone(),
            self.graph.clone(),
        );

        let query = HybridQuery::new(query_text)
            .with_anchors(anchors)
            .with_limit(self.config.max_items_per_section * 2);

        let hybrid_results = hybrid.search(query).unwrap_or_default();

        let nodes: Vec<Node> = if !hybrid_results.is_empty() {
            let mut candidates: Vec<Node> = hybrid_results
                .into_iter()
                .map(|r| r.node)
                .filter(|n| !seen.contains(&n.id))
                .collect();
            candidates = self.rank(candidates);
            candidates.truncate(self.config.max_items_per_section);
            candidates
        } else {
            let candidates: Vec<Node> = recent
                .into_iter()
                .filter(|n| !seen.contains(&n.id))
                .collect();
            let mut ranked = self.rank(candidates);
            ranked.truncate(self.config.max_items_per_section);
            ranked
        };

        Ok(BriefingSection {
            title: "Active Context".to_string(),
            nodes,
        })
    }

    fn generate_reviewable(
        &self,
        agent_node_id: NodeId,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let title = self.role_section_title("reviewable", &self.config.roles.reviewable);
        let kind_filters: Vec<NodeKind> = self
            .config
            .roles
            .reviewable
            .iter()
            .filter_map(|k| NodeKind::new(k).ok())
            .collect();

        let result = self.graph.traverse(TraversalRequest {
            start: vec![agent_node_id],
            max_depth: Some(2),
            direction: TraversalDirection::Both,
            relation_filter: Some(vec![
                Relation::new("applies_to").unwrap(),
                Relation::new("instance_of").unwrap(),
            ]),
            kind_filter: Some(kind_filters),
            ..Default::default()
        })?;

        let candidates: Vec<Node> = result
            .nodes
            .into_values()
            .filter(|n| n.id != agent_node_id && !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection { title, nodes })
    }

    fn generate_trackable(
        &self,
        agent_node_id: NodeId,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let title = self.role_section_title("trackable", &self.config.roles.trackable);
        let kind_filters: Vec<NodeKind> = self
            .config
            .roles
            .trackable
            .iter()
            .filter_map(|k| NodeKind::new(k).ok())
            .collect();

        let result = self.graph.traverse(TraversalRequest {
            start: vec![agent_node_id],
            max_depth: Some(2),
            direction: TraversalDirection::Both,
            kind_filter: Some(kind_filters),
            ..Default::default()
        })?;

        let candidates: Vec<Node> = result
            .nodes
            .into_values()
            .filter(|n| n.id != agent_node_id && !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection { title, nodes })
    }

    fn generate_unresolved(
        &self,
        agent_node_id: NodeId,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        // Traverse the immediate neighbourhood (depth 3, all relations) to find
        // nodes the agent can reach. Then filter in-memory for those involved in
        // Contradicts edges — this avoids a second traversal pass.
        let subgraph = self.graph.traverse(TraversalRequest {
            start: vec![agent_node_id],
            max_depth: Some(3),
            direction: TraversalDirection::Both,
            ..Default::default()
        })?;

        // Collect node IDs that appear on either side of a Contradicts edge
        let contradicting_ids: HashSet<NodeId> = subgraph
            .edges
            .iter()
            .filter(|e| e.relation.as_str() == "contradicts")
            .flat_map(|e| [e.from, e.to])
            .collect();

        let candidates: Vec<Node> = subgraph
            .nodes
            .into_values()
            .filter(|n| {
                n.id != agent_node_id && !seen.contains(&n.id) && contradicting_ids.contains(&n.id)
            })
            .collect();

        // No importance filter for contradictions — surface them regardless of score
        let mut nodes = candidates;
        nodes.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Unresolved Contradictions".to_string(),
            nodes,
        })
    }

    fn generate_temporal(
        &self,
        agent_id: &str,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let title = self.role_section_title("temporal", &self.config.roles.temporal);
        let kind_filters: Vec<NodeKind> = self
            .config
            .roles
            .temporal
            .iter()
            .filter_map(|k| NodeKind::new(k).ok())
            .collect();

        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        // Try agent-specific first, fall back to global
        let mut raw = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(agent_id.to_string())
                .with_kinds(kind_filters.clone())
                .created_after(cutoff)
                .with_limit(self.config.max_items_per_section * 2),
        )?;

        if raw.is_empty() {
            raw = self.storage.list_nodes(
                NodeFilter::new()
                    .with_kinds(kind_filters)
                    .created_after(cutoff)
                    .with_limit(self.config.max_items_per_section * 2),
            )?;
        }

        let candidates: Vec<Node> = raw.into_iter().filter(|n| !seen.contains(&n.id)).collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection { title, nodes })
    }

    /// Global fallback: query nodes by kind without requiring graph traversal.
    /// Used when no agent node exists in the graph.
    fn generate_global_by_kind(
        &self,
        kind: &str,
        section_title: &str,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let candidates: Vec<Node> = self
            .storage
            .list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![NodeKind::new(kind).unwrap()])
                    .with_min_importance(self.config.min_importance)
                    .with_limit(self.config.max_items_per_section * 2),
            )?
            .into_iter()
            .filter(|n| !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: section_title.to_string(),
            nodes,
        })
    }

    /// Phase 2: Generate sections for node kinds not covered by the default
    /// structured generators. Uses `generate_global_by_kind` for each novel kind.
    fn generate_auto_discovered_sections(
        &self,
        seen: &HashSet<NodeId>,
    ) -> Result<Vec<BriefingSection>> {
        let all_kinds = self.storage.list_distinct_kinds()?;

        let mapped = self.config.roles.mapped_kinds();

        let excluded: HashSet<&str> = self
            .config
            .exclude_kinds
            .iter()
            .map(|s| s.as_str())
            .collect();

        let novel_kinds: Vec<&NodeKind> = all_kinds
            .iter()
            .filter(|k| !mapped.contains(k.as_str()))
            .filter(|k| !excluded.contains(k.as_str()))
            .collect();

        let mut sections = Vec::new();

        for kind in novel_kinds {
            let title = kind_to_section_title(kind.as_str());
            let section = self.generate_global_by_kind(kind.as_str(), &title, seen)?;

            if !section.nodes.is_empty() {
                sections.push(section);
            }
        }

        // Sort sections: most total importance first
        sections.sort_by(|a, b| {
            let a_imp: f32 = a.nodes.iter().map(|n| n.importance).sum();
            let b_imp: f32 = b.nodes.iter().map(|n| n.importance).sum();
            b_imp
                .partial_cmp(&a_imp)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(sections)
    }

    // ── Scope::Shared ─────────────────────────────────────────────────────

    /// Generate an agent-scoped briefing, then append a cross-agent context
    /// section showing other agents' knowledge about shared entities.
    fn generate_shared(&self, agent_id: &str) -> Result<Briefing> {
        let mut briefing = self.generate(agent_id)?;
        // `generate` may return a cached result — we always need fresh cross-agent data.
        briefing.cached = false;

        let seen: HashSet<NodeId> = briefing
            .sections
            .iter()
            .flat_map(|s| s.nodes.iter().map(|n| n.id))
            .collect();

        let agent_node_id = self.find_agent_node(agent_id)?;
        let cross = self.generate_cross_agent_context(agent_id, agent_node_id, &seen)?;
        if !cross.nodes.is_empty() {
            briefing.nodes_consulted += cross.nodes.len();
            briefing.sections.push(cross);
        }

        Ok(briefing)
    }

    /// Build the "Cross-agent context" section.
    ///
    /// Two-hop traversal: agent's recent nodes → referenced entities → other agents' nodes.
    fn generate_cross_agent_context(
        &self,
        agent_id: &str,
        _agent_node_id: Option<NodeId>,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        // 1. Collect entity nodes referenced by this agent's recent knowledge
        let recent_nodes = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(agent_id.to_string())
                .created_after(cutoff)
                .with_limit(50),
        )?;

        let mut entity_ids: HashSet<NodeId> = HashSet::new();
        for node in &recent_nodes {
            let outbound = self.storage.edges_from(node.id)?;
            for edge in outbound {
                if edge.relation.as_str() == "references" {
                    entity_ids.insert(edge.to);
                }
            }
        }

        if entity_ids.is_empty() {
            return Ok(BriefingSection {
                title: "Cross-agent context".to_string(),
                nodes: vec![],
            });
        }

        // 2. From entity nodes, find OTHER agents' knowledge about those entities
        let mut cross_nodes: Vec<Node> = Vec::new();
        for entity_id in &entity_ids {
            let inbound = self.storage.edges_to(*entity_id)?;
            for edge in inbound {
                if edge.relation.as_str() == "references" {
                    if let Ok(Some(node)) = self.storage.get_node(edge.from) {
                        if node.source.agent != agent_id
                            && !seen.contains(&node.id)
                            && !node.deleted
                        {
                            cross_nodes.push(node);
                        }
                    }
                }
            }
        }

        // 3. Rank and truncate
        let mut ranked = self.rank(cross_nodes);
        ranked.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Cross-agent context".to_string(),
            nodes: ranked,
        })
    }

    // ── Scope::Unified ────────────────────────────────────────────────────

    /// Multi-agent unified briefing for orchestrators.
    fn generate_unified(&self, agent_ids: &[String]) -> Result<Briefing> {
        let mut all_sections: Vec<BriefingSection> = Vec::new();
        let mut seen: HashSet<NodeId> = HashSet::new();

        // 1. For each agent, generate a summary section
        for agent_id in agent_ids {
            let agent_node_id = self.find_agent_node(agent_id)?;
            let summary = self.generate_agent_summary(agent_id, agent_node_id, &seen)?;
            for n in &summary.nodes {
                seen.insert(n.id);
            }
            if !summary.nodes.is_empty() {
                all_sections.push(summary);
            }
        }

        // 2. Find shared entities (referenced by 2+ of the listed agents)
        let shared_entities = self.find_shared_entities(agent_ids)?;
        if !shared_entities.nodes.is_empty() {
            for n in &shared_entities.nodes {
                seen.insert(n.id);
            }
            all_sections.push(shared_entities);
        }

        // 3. Contradictions across agents
        let cross_contradictions = self.find_cross_agent_contradictions(agent_ids, &seen)?;
        if !cross_contradictions.nodes.is_empty() {
            all_sections.push(cross_contradictions);
        }

        // Enforce max_total_items
        let mut total = 0usize;
        for section in &mut all_sections {
            let remaining = self.config.max_total_items.saturating_sub(total);
            section.nodes.truncate(remaining);
            total += section.nodes.len();
        }
        all_sections.retain(|s| !s.nodes.is_empty());

        let nodes_consulted = all_sections.iter().map(|s| s.nodes.len()).sum();

        Ok(Briefing {
            agent_id: agent_ids.join(","),
            generated_at: Utc::now(),
            nodes_consulted,
            sections: all_sections,
            cached: false,
        })
    }

    /// Generate a summary section for one agent in a unified briefing.
    fn generate_agent_summary(
        &self,
        agent_id: &str,
        agent_node_id: Option<NodeId>,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        let mut nodes: Vec<Node> = Vec::new();

        // Include the agent node itself if present
        if let Some(aid) = agent_node_id {
            if let Ok(Some(agent_node)) = self.storage.get_node(aid) {
                if !seen.contains(&agent_node.id) {
                    nodes.push(agent_node);
                }
            }
        }

        // Pull the agent's recent high-importance nodes
        let recent = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(agent_id.to_string())
                .created_after(cutoff)
                .with_min_importance(self.config.min_importance)
                .with_limit(self.config.max_items_per_section * 2),
        )?;

        let candidates: Vec<Node> = recent
            .into_iter()
            .filter(|n| !seen.contains(&n.id) && !nodes.iter().any(|e| e.id == n.id))
            .collect();

        let mut ranked = self.rank(candidates);
        ranked.truncate(
            self.config
                .max_items_per_section
                .saturating_sub(nodes.len()),
        );
        nodes.extend(ranked);

        Ok(BriefingSection {
            title: format!("Agent: {}", agent_id),
            nodes,
        })
    }

    /// Find entity nodes referenced by 2+ of the listed agents.
    fn find_shared_entities(&self, agent_ids: &[String]) -> Result<BriefingSection> {
        // For each agent, collect entity IDs they reference
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        let mut entity_agent_count: HashMap<NodeId, HashSet<String>> = HashMap::new();

        for agent_id in agent_ids {
            let nodes = self.storage.list_nodes(
                NodeFilter::new()
                    .with_source_agent(agent_id.to_string())
                    .created_after(cutoff)
                    .with_limit(50),
            )?;

            for node in &nodes {
                let outbound = self.storage.edges_from(node.id)?;
                for edge in outbound {
                    if edge.relation.as_str() == "references" {
                        entity_agent_count
                            .entry(edge.to)
                            .or_default()
                            .insert(agent_id.clone());
                    }
                }
            }
        }

        // Keep only entities referenced by 2+ agents
        let shared_ids: Vec<NodeId> = entity_agent_count
            .into_iter()
            .filter(|(_, agents)| agents.len() >= 2)
            .map(|(id, _)| id)
            .collect();

        let mut nodes: Vec<Node> = Vec::new();
        for id in shared_ids {
            if let Ok(Some(node)) = self.storage.get_node(id) {
                if !node.deleted {
                    nodes.push(node);
                }
            }
        }

        let mut ranked = self.rank(nodes);
        ranked.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Shared entities".to_string(),
            nodes: ranked,
        })
    }

    /// Find nodes involved in contradictions across different agents.
    /// Note: `_seen` is accepted for interface consistency but not used for filtering
    /// because contradictions should be surfaced even if the nodes already appeared
    /// in per-agent summary sections.
    fn find_cross_agent_contradictions(
        &self,
        agent_ids: &[String],
        _seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        if !self.config.include_contradictions {
            return Ok(BriefingSection {
                title: "Cross-agent contradictions".to_string(),
                nodes: vec![],
            });
        }

        let agent_set: HashSet<&str> = agent_ids.iter().map(|s| s.as_str()).collect();
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        // Collect recent nodes from all listed agents
        let mut all_node_ids: HashSet<NodeId> = HashSet::new();
        for agent_id in agent_ids {
            let nodes = self.storage.list_nodes(
                NodeFilter::new()
                    .with_source_agent(agent_id.to_string())
                    .created_after(cutoff)
                    .with_limit(50),
            )?;
            for n in &nodes {
                all_node_ids.insert(n.id);
            }
        }

        // Find contradiction edges where the two sides come from different listed agents
        let mut contradicting_nodes: Vec<Node> = Vec::new();
        for node_id in &all_node_ids {
            let outbound = self.storage.edges_from(*node_id)?;
            for edge in &outbound {
                if edge.relation.as_str() == "contradicts" {
                    if let Ok(Some(other)) = self.storage.get_node(edge.to) {
                        if agent_set.contains(other.source.agent.as_str())
                            && !other.deleted
                        {
                            // Verify they're from *different* agents
                            if let Ok(Some(this)) = self.storage.get_node(*node_id) {
                                if this.source.agent != other.source.agent {
                                    contradicting_nodes.push(other);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Dedup by node ID
        let mut deduped: Vec<Node> = Vec::new();
        let mut dedup_seen: HashSet<NodeId> = HashSet::new();
        for n in contradicting_nodes {
            if dedup_seen.insert(n.id) {
                deduped.push(n);
            }
        }

        deduped.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        deduped.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Cross-agent contradictions".to_string(),
            nodes: deduped,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphEngineImpl;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, Source};
    use crate::vector::{SimilarityResult, VectorFilter};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;
    use tempfile::TempDir;

    // --- Minimal mock implementations that never download a model ---

    #[derive(Clone)]
    struct MockEmbedder;

    impl EmbeddingService for MockEmbedder {
        fn embed(&self, _text: &str) -> crate::error::Result<crate::types::Embedding> {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }
        fn embed_batch(
            &self,
            texts: &[String],
        ) -> crate::error::Result<Vec<crate::types::Embedding>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
        fn dimension(&self) -> usize {
            4
        }
        fn model_name(&self) -> &str {
            "mock"
        }
    }

    #[derive(Clone)]
    struct MockVectorIndex;

    impl crate::vector::VectorIndex for MockVectorIndex {
        fn insert(
            &mut self,
            _id: crate::types::NodeId,
            _embedding: &crate::types::Embedding,
        ) -> crate::error::Result<()> {
            Ok(())
        }
        fn remove(&mut self, _id: crate::types::NodeId) -> crate::error::Result<()> {
            Ok(())
        }
        fn search(
            &self,
            _query: &crate::types::Embedding,
            _k: usize,
            _filter: Option<&VectorFilter>,
        ) -> crate::error::Result<Vec<SimilarityResult>> {
            Ok(vec![])
        }
        fn search_threshold(
            &self,
            _query: &crate::types::Embedding,
            _threshold: f32,
            _filter: Option<&VectorFilter>,
        ) -> crate::error::Result<Vec<SimilarityResult>> {
            Ok(vec![])
        }
        fn search_batch(
            &self,
            queries: &[(crate::types::NodeId, crate::types::Embedding)],
            _k: usize,
            _filter: Option<&VectorFilter>,
        ) -> crate::error::Result<HashMap<crate::types::NodeId, Vec<SimilarityResult>>> {
            Ok(queries.iter().map(|(id, _)| (*id, vec![])).collect())
        }
        fn len(&self) -> usize {
            0
        }
        fn rebuild(&mut self) -> crate::error::Result<()> {
            Ok(())
        }
        fn save(&self, _path: &Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn load(_path: &Path) -> crate::error::Result<Self> {
            Ok(MockVectorIndex)
        }
    }

    type TestEngine = BriefingEngine<
        RedbStorage,
        MockEmbedder,
        MockVectorIndex,
        Arc<GraphEngineImpl<RedbStorage>>,
    >;

    fn make_engine(storage: Arc<RedbStorage>) -> (TestEngine, Arc<AtomicU64>) {
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let graph_version = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(
            storage,
            graph,
            MockVectorIndex,
            MockEmbedder,
            graph_version.clone(),
            BriefingConfig::default(),
        );
        (engine, graph_version)
    }

    fn make_node(kind: NodeKind, title: &str, agent: &str) -> Node {
        Node::new(
            kind,
            title.to_string(),
            title.to_string(),
            Source {
                agent: agent.to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    fn manual_edge(from: NodeId, to: NodeId, rel: Relation) -> Edge {
        Edge::new(
            from,
            to,
            rel,
            1.0,
            EdgeProvenance::Manual {
                created_by: "test".into(),
            },
        )
    }

    // Test 1: identity section surfaces preference nodes
    #[test]
    fn test_identity_section_includes_preferences() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let pref = make_node(NodeKind::new("preference").unwrap(), "Prefers async", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&pref).unwrap();
        storage
            .put_edge(&manual_edge(
                pref.id,
                agent.id,
                Relation::new("applies_to").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Identity & Preferences")
            .expect("identity section missing");

        assert!(
            section
                .nodes
                .iter()
                .any(|n| n.kind == NodeKind::new("preference").unwrap()),
            "Preference node not found in identity section"
        );
    }

    // Test 2: active context returns recent nodes (mock vector returns empty → falls back)
    #[test]
    fn test_active_context_uses_recent_nodes() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let fact = make_node(NodeKind::new("fact").unwrap(), "Recent fact", "kai");
        storage.put_node(&fact).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let total: usize = briefing.sections.iter().map(|s| s.nodes.len()).sum();
        assert!(total > 0, "Expected at least one node in briefing");
    }

    // Test 3: pattern nodes discovered via AppliesTo traversal
    #[test]
    fn test_patterns_section_traverses_applies_to() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let pattern = make_node(
            NodeKind::new("pattern").unwrap(),
            "Recurring pattern",
            "kai",
        );
        storage.put_node(&agent).unwrap();
        storage.put_node(&pattern).unwrap();
        storage
            .put_edge(&manual_edge(
                pattern.id,
                agent.id,
                Relation::new("applies_to").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Patterns")
            .expect("Patterns section missing");

        assert!(
            !section.nodes.is_empty(),
            "Patterns section should not be empty"
        );
        assert!(section
            .nodes
            .iter()
            .any(|n| n.kind == NodeKind::new("pattern").unwrap()));
    }

    // Test 4: contradictions surface in unresolved section
    #[test]
    fn test_unresolved_surfaces_contradictions() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let fact1 = make_node(NodeKind::new("fact").unwrap(), "Fact A", "kai");
        let fact2 = make_node(NodeKind::new("fact").unwrap(), "Fact B", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&fact1).unwrap();
        storage.put_node(&fact2).unwrap();

        // Agent knows about fact1; fact1 contradicts fact2
        storage
            .put_edge(&manual_edge(
                agent.id,
                fact1.id,
                Relation::new("informed_by").unwrap(),
            ))
            .unwrap();
        storage
            .put_edge(&manual_edge(
                fact1.id,
                fact2.id,
                Relation::new("contradicts").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Unresolved Contradictions")
            .expect("Unresolved section missing");

        assert!(
            !section.nodes.is_empty(),
            "Unresolved section should contain contradicting nodes"
        );
    }

    // Test 5: max_items_per_section is enforced
    #[test]
    fn test_max_items_per_section_enforced() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        for i in 0..20 {
            let pref = make_node(
                NodeKind::new("preference").unwrap(),
                &format!("Pref {}", i),
                "kai",
            );
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(
                    pref.id,
                    agent.id,
                    Relation::new("applies_to").unwrap(),
                ))
                .unwrap();
        }

        let config = BriefingConfig {
            max_items_per_section: 5,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        for section in &briefing.sections {
            assert!(
                section.nodes.len() <= 5,
                "Section '{}' has {} items, max is 5",
                section.title,
                section.nodes.len()
            );
        }
    }

    // Test 6: max_total_items caps the grand total
    #[test]
    fn test_max_total_items_enforced() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        for i in 0..30 {
            let pref = make_node(
                NodeKind::new("preference").unwrap(),
                &format!("Pref {}", i),
                "kai",
            );
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(
                    pref.id,
                    agent.id,
                    Relation::new("applies_to").unwrap(),
                ))
                .unwrap();
        }

        let config = BriefingConfig {
            max_items_per_section: 20,
            max_total_items: 10,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        let total: usize = briefing.sections.iter().map(|s| s.nodes.len()).sum();
        assert!(total <= 10, "Total {} exceeds max_total_items 10", total);
    }

    // Test 7: renderer truncates at max_chars
    #[test]
    fn test_max_chars_truncation() {
        use super::super::renderer::MarkdownRenderer;
        use super::super::BriefingSection;

        let briefing = Briefing {
            agent_id: "test".to_string(),
            generated_at: Utc::now(),
            nodes_consulted: 1,
            sections: vec![BriefingSection {
                title: "Test".to_string(),
                nodes: vec![make_node(
                    NodeKind::new("fact").unwrap(),
                    "A fact with a rather long title",
                    "test",
                )],
            }],
            cached: false,
        };

        let renderer = MarkdownRenderer { max_chars: 50 };
        let rendered = renderer.render(&briefing);
        assert!(
            rendered.len() <= 50,
            "Rendered length {} > 50",
            rendered.len()
        );
    }

    // Test 8: second call returns cached briefing
    #[test]
    fn test_cache_returns_cached_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        let (engine, _) = make_engine(storage);

        let b1 = engine.generate("kai").unwrap();
        assert!(!b1.cached, "First call must not be cached");

        let b2 = engine.generate("kai").unwrap();
        assert!(b2.cached, "Second call with same version must be cached");
    }

    // Test 9: version increment invalidates cache
    #[test]
    fn test_cache_invalidates_on_version_increment() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        let (engine, version) = make_engine(storage);

        let b1 = engine.generate("kai").unwrap();
        assert!(!b1.cached);

        version.fetch_add(1, Ordering::Relaxed);

        let b2 = engine.generate("kai").unwrap();
        assert!(!b2.cached, "After version bump, cache must be invalid");
    }

    // Test 10: access_count incremented after briefing is served
    #[test]
    fn test_access_tracking_increments_count() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        let (engine, _) = make_engine(storage.clone());
        engine.generate("kai").unwrap();

        let updated = storage.get_node(agent.id).unwrap().unwrap();
        assert!(
            updated.access_count > 0,
            "access_count should be > 0 after briefing"
        );
    }

    // Test 11: markdown output has ## headers and bullet points
    #[test]
    fn test_markdown_rendering_valid() {
        use super::super::renderer::MarkdownRenderer;
        use super::super::BriefingSection;

        let briefing = Briefing {
            agent_id: "kai".to_string(),
            generated_at: Utc::now(),
            nodes_consulted: 1,
            sections: vec![BriefingSection {
                title: "Identity & Preferences".to_string(),
                nodes: vec![make_node(
                    NodeKind::new("agent").unwrap(),
                    "Kai Agent",
                    "kai",
                )],
            }],
            cached: false,
        };

        let rendered = MarkdownRenderer { max_chars: 8000 }.render(&briefing);

        assert!(rendered.contains("# Briefing:"), "missing top-level title");
        assert!(
            rendered.contains("## Identity & Preferences"),
            "missing ## header"
        );
        assert!(rendered.contains("- **"), "missing bold bullet");
    }

    // Test 12: compact renderer fits within max_chars
    #[test]
    fn test_compact_rendering_fits_limit() {
        use super::super::renderer::CompactRenderer;
        use super::super::BriefingSection;

        let briefing = Briefing {
            agent_id: "kai".to_string(),
            generated_at: Utc::now(),
            nodes_consulted: 5,
            sections: vec![BriefingSection {
                title: "Section".to_string(),
                nodes: (0..5)
                    .map(|i| {
                        make_node(
                            NodeKind::new("fact").unwrap(),
                            &format!("Long fact title number {}", i),
                            "kai",
                        )
                    })
                    .collect(),
            }],
            cached: false,
        };

        let rendered = CompactRenderer { max_chars: 200 }.render(&briefing);
        assert!(
            rendered.len() <= 200,
            "Compact output {} > 200",
            rendered.len()
        );
    }

    // Test 13: goals section populates via graph traversal
    #[test]
    fn test_goals_section_populates() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let goal = make_node(NodeKind::new("goal").unwrap(), "Ship Cortex v1", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&goal).unwrap();
        storage
            .put_edge(&manual_edge(
                agent.id,
                goal.id,
                Relation::new("informed_by").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Goals")
            .expect("Goals section missing");

        assert!(section
            .nodes
            .iter()
            .any(|n| n.kind == NodeKind::new("goal").unwrap()));
    }

    // Test 14: recent events section populates (or events appear in Active Context)
    //
    // Active Context runs before Recent Events, so a sole Event node may be
    // captured there first (by design — both cover the same time window).
    // The test verifies the event appears somewhere in the briefing.
    #[test]
    fn test_recent_events_section_populates() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let event = make_node(NodeKind::new("event").unwrap(), "Deployed to prod", "kai");
        storage.put_node(&event).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let all_nodes: Vec<&Node> = briefing.sections.iter().flat_map(|s| &s.nodes).collect();
        assert!(
            all_nodes
                .iter()
                .any(|n| n.kind == NodeKind::new("event").unwrap()),
            "Event node should appear in some section of the briefing"
        );
    }

    // Test 14b: Recent Events section gets events not captured by Active Context
    #[test]
    fn test_recent_events_section_overflow_from_active_context() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let config = BriefingConfig {
            max_items_per_section: 2,
            ..Default::default()
        };

        // Create 5 Events — Active Context will claim 2, Recent Events gets the rest
        for i in 0..5 {
            let ev = make_node(
                NodeKind::new("event").unwrap(),
                &format!("Event {}", i),
                "kai",
            );
            storage.put_node(&ev).unwrap();
        }

        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        let has_recent_events = briefing
            .sections
            .iter()
            .any(|s| s.title == "Recent Events" && !s.nodes.is_empty());

        assert!(has_recent_events, "Recent Events section should be non-empty when there are more events than Active Context can hold");
    }

    // Test 15: min_importance filter removes low-quality nodes
    #[test]
    fn test_min_importance_filters_low_quality_nodes() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        // High-importance pref
        let mut good_pref = make_node(NodeKind::new("preference").unwrap(), "Good pref", "kai");
        good_pref.importance = 0.9;
        // Low-importance pref — should be filtered
        let mut bad_pref = make_node(NodeKind::new("preference").unwrap(), "Bad pref", "kai");
        bad_pref.importance = 0.1;

        storage.put_node(&agent).unwrap();
        storage.put_node(&good_pref).unwrap();
        storage.put_node(&bad_pref).unwrap();
        storage
            .put_edge(&manual_edge(
                good_pref.id,
                agent.id,
                Relation::new("applies_to").unwrap(),
            ))
            .unwrap();
        storage
            .put_edge(&manual_edge(
                bad_pref.id,
                agent.id,
                Relation::new("applies_to").unwrap(),
            ))
            .unwrap();

        let config = BriefingConfig {
            min_importance: 0.5,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        let all_nodes: Vec<&Node> = briefing.sections.iter().flat_map(|s| &s.nodes).collect();
        assert!(
            all_nodes.iter().any(|n| n.data.title == "Good pref"),
            "High-importance preference should appear"
        );
        assert!(
            !all_nodes.iter().any(|n| n.data.title == "Bad pref"),
            "Low-importance preference must be filtered out"
        );
    }

    // Test 16: nodes within a section are sorted by importance descending
    #[test]
    fn test_nodes_sorted_by_importance_desc() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        // Create preferences with known importance values in non-sorted order
        for (i, importance) in [(0, 0.4f32), (1, 0.9f32), (2, 0.6f32)] {
            let mut pref = make_node(
                NodeKind::new("preference").unwrap(),
                &format!("Pref {}", i),
                "kai",
            );
            pref.importance = importance;
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(
                    pref.id,
                    agent.id,
                    Relation::new("applies_to").unwrap(),
                ))
                .unwrap();
        }

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let identity = briefing
            .sections
            .iter()
            .find(|s| s.title == "Identity & Preferences")
            .expect("Identity section missing");

        // The preferences in the section must be ordered high→low importance
        // (Agent node itself is first and excluded from this check)
        let pref_importances: Vec<f32> = identity
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::new("preference").unwrap())
            .map(|n| n.importance)
            .collect();

        for window in pref_importances.windows(2) {
            assert!(
                window[0] >= window[1],
                "Preferences not sorted by importance desc: {:?}",
                pref_importances
            );
        }
    }

    // Test 17: graceful degradation when no agent node exists
    #[test]
    fn test_fallback_identity_no_agent_node() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // No Agent node — only raw facts from the agent
        let fact = make_node(NodeKind::new("fact").unwrap(), "Some fact", "kai");
        storage.put_node(&fact).unwrap();

        let (engine, _) = make_engine(storage);
        // Should not panic or error; should return a briefing (possibly empty or with facts)
        let briefing = engine.generate("kai").unwrap();
        // At minimum, we should get back a valid (possibly empty) briefing struct
        assert_eq!(briefing.agent_id, "kai");
    }

    // Test 18: unicode content doesn't panic in renderer
    #[test]
    fn test_renderer_unicode_no_panic() {
        use super::super::renderer::MarkdownRenderer;

        let mut node = make_node(NodeKind::new("fact").unwrap(), "日本語タイトル", "test");
        node.data.body = "これは長いボディです。".repeat(30); // > 200 chars

        let briefing = Briefing {
            agent_id: "test".to_string(),
            generated_at: Utc::now(),
            nodes_consulted: 1,
            sections: vec![BriefingSection {
                title: "Facts".to_string(),
                nodes: vec![node],
            }],
            cached: false,
        };

        // These must not panic (byte-slicing multi-byte chars would panic)
        let full = MarkdownRenderer { max_chars: 8000 }.render(&briefing);
        let tiny = MarkdownRenderer { max_chars: 10 }.render(&briefing);
        assert!(!full.is_empty());
        assert!(tiny.chars().count() <= 10);
    }

    // Test 19: empty graph returns empty but valid briefing
    #[test]
    fn test_briefing_empty_graph() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("nobody").unwrap();

        assert_eq!(briefing.agent_id, "nobody");
        assert_eq!(briefing.nodes_consulted, 0);
        assert!(briefing.sections.is_empty());
    }

    // Test 20: access tracking uses record_access (access_count increments)
    #[test]
    fn test_access_tracking_uses_record_access() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let pref = make_node(NodeKind::new("preference").unwrap(), "A preference", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&pref).unwrap();
        storage
            .put_edge(&manual_edge(
                pref.id,
                agent.id,
                Relation::new("applies_to").unwrap(),
            ))
            .unwrap();

        let initial_agent_count = storage.get_node(agent.id).unwrap().unwrap().access_count;
        let initial_pref_count = storage.get_node(pref.id).unwrap().unwrap().access_count;

        let (engine, _) = make_engine(storage.clone());
        engine.generate("kai").unwrap();

        let updated_agent = storage.get_node(agent.id).unwrap().unwrap();
        let updated_pref = storage.get_node(pref.id).unwrap().unwrap();

        assert_eq!(updated_agent.access_count, initial_agent_count + 1);
        assert_eq!(updated_pref.access_count, initial_pref_count + 1);
    }

    // ====================================================================
    // Auto-Discovery Tests
    // ====================================================================

    // Test 21: kind_to_section_title derivation
    #[test]
    fn test_kind_to_section_title() {
        assert_eq!(kind_to_section_title("opportunity"), "Opportunities");
        assert_eq!(kind_to_section_title("experiment"), "Experiments");
        assert_eq!(kind_to_section_title("test_file"), "Test Files");
        assert_eq!(kind_to_section_title("class"), "Classes");
        assert_eq!(kind_to_section_title("insight"), "Insights");
        assert_eq!(kind_to_section_title("status"), "Statuses");
        assert_eq!(kind_to_section_title("function"), "Functions");
        assert_eq!(kind_to_section_title("research_note"), "Research Notes");
    }

    // Test 22: pluralise edge cases
    #[test]
    fn test_pluralise() {
        assert_eq!(pluralise("Key"), "Keys");
        assert_eq!(pluralise("Day"), "Days");
        assert_eq!(pluralise("Boy"), "Boys");
        assert_eq!(pluralise("Category"), "Categories");
        assert_eq!(pluralise("Bus"), "Buses");
        assert_eq!(pluralise("Box"), "Boxes");
        assert_eq!(pluralise("Wish"), "Wishes");
        assert_eq!(pluralise("Match"), "Matches");
        assert_eq!(pluralise("Node"), "Nodes");
    }

    // Test 23: graph with only default kinds produces no auto-discovered sections
    #[test]
    fn test_auto_discovery_default_kinds_only() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let fact = make_node(NodeKind::new("fact").unwrap(), "A fact", "kai");
        let pattern = make_node(NodeKind::new("pattern").unwrap(), "A pattern", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&fact).unwrap();
        storage.put_node(&pattern).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        // No section titles should be auto-derived from default kinds
        let auto_titles: Vec<&str> = briefing
            .sections
            .iter()
            .map(|s| s.title.as_str())
            .filter(|t| {
                !matches!(
                    *t,
                    "Identity & Preferences"
                        | "Patterns"
                        | "Goals"
                        | "Unresolved Contradictions"
                        | "Active Context"
                        | "Recent Events"
                        | "Key Decisions"
                )
            })
            .collect();

        assert!(
            auto_titles.is_empty(),
            "Expected no auto-discovered sections, found: {:?}",
            auto_titles
        );
    }

    // Test 24: novel kind produces auto-discovered section
    #[test]
    fn test_auto_discovery_novel_kind() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Test A/B", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Experiments")
            .expect("Auto-discovered 'Experiments' section missing");

        assert_eq!(section.nodes.len(), 1);
        assert_eq!(section.nodes[0].data.title, "Test A/B");
    }

    // Test 25: multiple novel kinds sorted by total importance
    #[test]
    fn test_auto_discovery_multiple_kinds_sorted() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Low importance kind
        let mut insight =
            make_node(NodeKind::new("insight").unwrap(), "Small insight", "kai");
        insight.importance = 0.4;
        storage.put_node(&insight).unwrap();

        // High importance kind
        let mut constraint = make_node(
            NodeKind::new("constraint").unwrap(),
            "Critical constraint",
            "kai",
        );
        constraint.importance = 0.9;
        storage.put_node(&constraint).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        // Find positions of auto-discovered sections
        let section_titles: Vec<&str> =
            briefing.sections.iter().map(|s| s.title.as_str()).collect();

        let constraints_pos = section_titles.iter().position(|t| *t == "Constraints");
        let insights_pos = section_titles.iter().position(|t| *t == "Insights");

        assert!(
            constraints_pos.is_some(),
            "Constraints section missing, sections: {:?}",
            section_titles
        );
        assert!(
            insights_pos.is_some(),
            "Insights section missing, sections: {:?}",
            section_titles
        );
        assert!(
            constraints_pos.unwrap() < insights_pos.unwrap(),
            "Higher-importance Constraints should come before Insights"
        );
    }

    // Test 26: novel kind below min_importance produces empty section (skipped)
    #[test]
    fn test_auto_discovery_skips_low_importance() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Low exp", "kai");
        experiment.importance = 0.1; // Below default min_importance of 0.3
        storage.put_node(&experiment).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        assert!(
            !briefing
                .sections
                .iter()
                .any(|s| s.title == "Experiments"),
            "Low-importance novel kind should not produce a section"
        );
    }

    // Test 27: exclude_kinds prevents section generation
    #[test]
    fn test_auto_discovery_exclude_kinds() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Test A/B", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        let config = BriefingConfig {
            exclude_kinds: vec!["experiment".to_string()],
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine =
            BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        assert!(
            !briefing
                .sections
                .iter()
                .any(|s| s.title == "Experiments"),
            "Excluded kind should not produce a section"
        );
    }

    // Test 28: default kind not duplicated in auto-discovery
    #[test]
    fn test_auto_discovery_no_duplicate_default_kinds() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Create nodes with default kinds
        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let goal = make_node(NodeKind::new("goal").unwrap(), "Ship v1", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&goal).unwrap();
        storage
            .put_edge(&manual_edge(
                agent.id,
                goal.id,
                Relation::new("informed_by").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        // Count how many times "Goals" appears as a section title
        let goals_count = briefing
            .sections
            .iter()
            .filter(|s| s.title == "Goals")
            .count();

        assert!(
            goals_count <= 1,
            "Default kind 'goal' should not appear as both a traversal section and auto-discovered section"
        );
    }

    // Test 29: seen_ids prevents nodes appearing in both Phase 1 and Phase 2
    #[test]
    fn test_auto_discovery_seen_ids_dedup() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Shared exp", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        // Collect all node IDs across all sections
        let all_ids: Vec<NodeId> = briefing
            .sections
            .iter()
            .flat_map(|s| s.nodes.iter().map(|n| n.id))
            .collect();

        let unique_ids: HashSet<NodeId> = all_ids.iter().copied().collect();
        assert_eq!(
            all_ids.len(),
            unique_ids.len(),
            "No node should appear in multiple sections"
        );
    }

    // Test 30: auto-discovered section appears before Active Context
    #[test]
    fn test_auto_discovery_before_active_context() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Create a novel-kind node and a fact (to populate Active Context)
        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Novel exp", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        let mut fact = make_node(NodeKind::new("fact").unwrap(), "A fact", "kai");
        fact.importance = 0.5;
        storage.put_node(&fact).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section_titles: Vec<&str> =
            briefing.sections.iter().map(|s| s.title.as_str()).collect();

        let experiments_pos = section_titles.iter().position(|t| *t == "Experiments");
        let active_pos = section_titles.iter().position(|t| *t == "Active Context");

        if let (Some(exp), Some(act)) = (experiments_pos, active_pos) {
            assert!(
                exp < act,
                "Auto-discovered section should come before Active Context"
            );
        }
    }

    // Test 31: list_distinct_kinds returns correct kinds
    #[test]
    fn test_list_distinct_kinds() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Empty graph
        let kinds = storage.list_distinct_kinds().unwrap();
        assert!(kinds.is_empty(), "Empty graph should have no kinds");

        // Add nodes
        let fact = make_node(NodeKind::new("fact").unwrap(), "A fact", "kai");
        storage.put_node(&fact).unwrap();

        let experiment =
            make_node(NodeKind::new("experiment").unwrap(), "An exp", "kai");
        storage.put_node(&experiment).unwrap();

        // Add a second fact — should not duplicate
        let fact2 = make_node(NodeKind::new("fact").unwrap(), "Another fact", "kai");
        storage.put_node(&fact2).unwrap();

        let kinds = storage.list_distinct_kinds().unwrap();
        let kind_strs: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();

        assert_eq!(kind_strs.len(), 2);
        assert!(kind_strs.contains(&"experiment"));
        assert!(kind_strs.contains(&"fact"));
    }

    // ====================================================================
    // Role Configuration Tests (Spec 14)
    // ====================================================================

    // Test 32: mapped_kinds returns union of all roles
    #[test]
    fn test_mapped_kinds_returns_all_role_kinds() {
        let config = BriefingRoleConfig::default();
        let mapped = config.mapped_kinds();
        assert!(mapped.contains("agent"));
        assert!(mapped.contains("preference"));
        assert!(mapped.contains("goal"));
        assert!(mapped.contains("event"));
        assert!(mapped.contains("pattern"));
        assert!(mapped.contains("fact"));
        assert!(mapped.contains("decision"));
        assert_eq!(mapped.len(), 7);
    }

    // Test 33: custom role config for coding agent produces correct section titles
    #[test]
    fn test_custom_roles_coding_agent() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "coder", "coder");
        storage.put_node(&agent).unwrap();

        // Create a "task" node and link it (trackable role in coding template)
        let mut task = make_node(NodeKind::new("task").unwrap(), "Fix bug #42", "coder");
        task.importance = 0.8;
        storage.put_node(&task).unwrap();
        storage
            .put_edge(&manual_edge(
                agent.id,
                task.id,
                Relation::new("informed_by").unwrap(),
            ))
            .unwrap();

        let config = BriefingConfig {
            roles: BriefingRoleConfig {
                identity: vec!["agent".into()],
                persistent: vec!["constraint".into()],
                trackable: vec!["task".into(), "milestone".into()],
                temporal: vec!["commit".into()],
                reviewable: vec!["pattern".into()],
                superseding: vec!["dependency".into()],
            },
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("coder").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Tasks & Milestones")
            .expect("Trackable section with multi-kind title missing");

        assert!(section
            .nodes
            .iter()
            .any(|n| n.data.title == "Fix bug #42"));
    }

    // Test 34: kinds not in any role appear in auto-discovered sections
    #[test]
    fn test_unmapped_kinds_auto_discovered() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Use a config where "experiment" is NOT mapped to any role
        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Novel exp", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        // Default roles don't include "experiment"
        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        assert!(
            briefing
                .sections
                .iter()
                .any(|s| s.title == "Experiments"),
            "Unmapped kind should appear as auto-discovered section"
        );
    }

    // Test 35: custom section titles from titles config
    #[test]
    fn test_custom_section_titles() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        let mut event = make_node(NodeKind::new("event").unwrap(), "Deploy v2", "kai");
        event.importance = 0.8;
        storage.put_node(&event).unwrap();

        let mut titles = HashMap::new();
        titles.insert("temporal".to_string(), "What Just Happened".to_string());

        let config = BriefingConfig {
            titles,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        assert!(
            briefing
                .sections
                .iter()
                .any(|s| s.title == "What Just Happened"),
            "Custom title override should be used"
        );
    }

    // Test 36: default role config matches DEFAULT_SECTION_KINDS
    #[test]
    fn test_default_mapped_kinds_match_legacy() {
        let config = BriefingRoleConfig::default();
        let mapped = config.mapped_kinds();

        let legacy = ["agent", "preference", "fact", "pattern", "goal", "event", "decision"];
        for kind in &legacy {
            assert!(
                mapped.contains(*kind),
                "Default mapped_kinds missing legacy kind: {}",
                kind
            );
        }
        assert_eq!(
            mapped.len(),
            legacy.len(),
            "Default mapped_kinds should have exactly the same kinds as legacy DEFAULT_SECTION_KINDS"
        );
    }

    // Test 37: role_section_title derives correct titles
    #[test]
    fn test_role_section_title_derivation() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());
        let (engine, _) = make_engine(storage);

        // identity always returns fixed title
        assert_eq!(
            engine.role_section_title("identity", &["agent".into()]),
            "Identity & Preferences"
        );

        // temporal with single kind
        assert_eq!(
            engine.role_section_title("temporal", &["event".into()]),
            "Recent Events"
        );

        // temporal with multiple kinds
        assert_eq!(
            engine.role_section_title("temporal", &["commit".into(), "deployment".into()]),
            "Recent Activity"
        );

        // single-kind role
        assert_eq!(
            engine.role_section_title("reviewable", &["pattern".into()]),
            "Patterns"
        );

        // multi-kind role
        assert_eq!(
            engine.role_section_title("trackable", &["task".into(), "milestone".into()]),
            "Tasks & Milestones"
        );
    }

    // Test 38: custom roles exclude mapped kinds from auto-discovery
    #[test]
    fn test_custom_roles_exclude_from_auto_discovery() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Create an agent so the traversal path is taken
        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        storage.put_node(&agent).unwrap();

        // "experiment" is mapped to reviewable — should NOT auto-discover
        let mut experiment =
            make_node(NodeKind::new("experiment").unwrap(), "Test A/B", "kai");
        experiment.importance = 0.8;
        storage.put_node(&experiment).unwrap();

        let config = BriefingConfig {
            roles: BriefingRoleConfig {
                reviewable: vec!["experiment".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine = BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        // Count sections titled "Experiments" — reviewable traversal may produce one
        // if agent is linked, but auto-discovery must NOT produce a duplicate.
        // Since experiment isn't linked to agent via applies_to/instance_of,
        // the reviewable traversal finds nothing — and auto-discovery is blocked.
        // The experiment may appear in Active Context, but not as its own section.
        let experiments_sections: Vec<_> = briefing
            .sections
            .iter()
            .filter(|s| s.title == "Experiments")
            .collect();

        assert!(
            experiments_sections.is_empty(),
            "Mapped kind should not produce an auto-discovered section"
        );
    }

    // ── Spec 15: Briefing scope tests ────────────────────────────────────

    // Test: Agent scope via generate_with_scope is identical to generate()
    #[test]
    fn test_agent_scope_identical_to_generate() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let fact = make_node(NodeKind::new("fact").unwrap(), "Some fact", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&fact).unwrap();

        let (engine, _) = make_engine(storage);

        let b1 = engine.generate("kai").unwrap();
        // Invalidate cache so we get fresh result
        let b2 = engine
            .generate_with_scope("kai", super::super::BriefingScope::Agent)
            .unwrap();

        assert_eq!(b1.sections.len(), b2.sections.len());
        assert_eq!(b1.nodes_consulted, b2.nodes_consulted);
        for (s1, s2) in b1.sections.iter().zip(b2.sections.iter()) {
            assert_eq!(s1.title, s2.title);
            assert_eq!(s1.nodes.len(), s2.nodes.len());
        }
    }

    // Test: Shared scope includes cross-agent context when entity edges exist
    #[test]
    fn test_shared_scope_includes_cross_agent_context() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        // Two agents: kai and scout
        let agent_kai = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let agent_scout = make_node(NodeKind::new("agent").unwrap(), "scout", "scout");

        // An entity node (e.g. a project)
        let entity = make_node(NodeKind::new("entity").unwrap(), "Project Alpha", "system");

        // Kai's fact references the entity
        let kai_fact = make_node(NodeKind::new("fact").unwrap(), "Kai's observation", "kai");
        // Scout's fact also references the entity
        let scout_fact = make_node(
            NodeKind::new("fact").unwrap(),
            "Scout's analysis",
            "scout",
        );

        storage.put_node(&agent_kai).unwrap();
        storage.put_node(&agent_scout).unwrap();
        storage.put_node(&entity).unwrap();
        storage.put_node(&kai_fact).unwrap();
        storage.put_node(&scout_fact).unwrap();

        // kai_fact -> references -> entity
        storage
            .put_edge(&manual_edge(
                kai_fact.id,
                entity.id,
                Relation::new("references").unwrap(),
            ))
            .unwrap();
        // scout_fact -> references -> entity
        storage
            .put_edge(&manual_edge(
                scout_fact.id,
                entity.id,
                Relation::new("references").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine
            .generate_with_scope("kai", super::super::BriefingScope::Shared)
            .unwrap();

        let cross_section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Cross-agent context");

        assert!(
            cross_section.is_some(),
            "Shared scope should include cross-agent context section"
        );
        let cross = cross_section.unwrap();
        assert!(
            cross
                .nodes
                .iter()
                .any(|n| n.data.title == "Scout's analysis"),
            "Cross-agent section should include Scout's node"
        );
        assert!(
            !cross
                .nodes
                .iter()
                .any(|n| n.data.title == "Kai's observation"),
            "Cross-agent section must NOT include the requesting agent's own nodes"
        );
    }

    // Test: Shared scope with no entity edges produces no cross-agent section
    #[test]
    fn test_shared_scope_empty_entity_graph() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let fact = make_node(NodeKind::new("fact").unwrap(), "Standalone fact", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&fact).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine
            .generate_with_scope("kai", super::super::BriefingScope::Shared)
            .unwrap();

        let cross_section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Cross-agent context");
        assert!(
            cross_section.is_none(),
            "No cross-agent section when no entity references exist"
        );
    }

    // Test: Unified scope produces sections for each agent
    #[test]
    fn test_unified_scope_produces_per_agent_sections() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let agent_kai = make_node(NodeKind::new("agent").unwrap(), "kai", "kai");
        let agent_scout = make_node(NodeKind::new("agent").unwrap(), "scout", "scout");
        let kai_fact = make_node(NodeKind::new("fact").unwrap(), "Kai fact", "kai");
        let scout_fact = make_node(NodeKind::new("fact").unwrap(), "Scout fact", "scout");

        storage.put_node(&agent_kai).unwrap();
        storage.put_node(&agent_scout).unwrap();
        storage.put_node(&kai_fact).unwrap();
        storage.put_node(&scout_fact).unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine
            .generate_with_scope(
                "kai",
                super::super::BriefingScope::Unified(vec![
                    "kai".to_string(),
                    "scout".to_string(),
                ]),
            )
            .unwrap();

        assert_eq!(briefing.agent_id, "kai,scout");

        let kai_section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Agent: kai");
        let scout_section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Agent: scout");

        assert!(
            kai_section.is_some(),
            "Unified briefing should have a section for kai"
        );
        assert!(
            scout_section.is_some(),
            "Unified briefing should have a section for scout"
        );
    }

    // Test: Unified scope includes shared entities section
    #[test]
    fn test_unified_scope_shared_entities() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let entity = make_node(NodeKind::new("entity").unwrap(), "Shared Resource", "system");
        let kai_fact = make_node(NodeKind::new("fact").unwrap(), "Kai ref", "kai");
        let scout_fact = make_node(NodeKind::new("fact").unwrap(), "Scout ref", "scout");

        storage.put_node(&entity).unwrap();
        storage.put_node(&kai_fact).unwrap();
        storage.put_node(&scout_fact).unwrap();

        // Both agents reference the same entity
        storage
            .put_edge(&manual_edge(
                kai_fact.id,
                entity.id,
                Relation::new("references").unwrap(),
            ))
            .unwrap();
        storage
            .put_edge(&manual_edge(
                scout_fact.id,
                entity.id,
                Relation::new("references").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine
            .generate_with_scope(
                "kai",
                super::super::BriefingScope::Unified(vec![
                    "kai".to_string(),
                    "scout".to_string(),
                ]),
            )
            .unwrap();

        let shared = briefing
            .sections
            .iter()
            .find(|s| s.title == "Shared entities");

        assert!(
            shared.is_some(),
            "Unified briefing should include shared entities section"
        );
        assert!(
            shared
                .unwrap()
                .nodes
                .iter()
                .any(|n| n.data.title == "Shared Resource"),
            "Shared entities should include the entity referenced by both agents"
        );
    }

    // Test: Unified scope cross-agent contradictions
    #[test]
    fn test_unified_scope_cross_agent_contradictions() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());

        let kai_fact = make_node(NodeKind::new("fact").unwrap(), "Kai says X", "kai");
        let scout_fact = make_node(NodeKind::new("fact").unwrap(), "Scout says not-X", "scout");

        storage.put_node(&kai_fact).unwrap();
        storage.put_node(&scout_fact).unwrap();

        // Cross-agent contradiction
        storage
            .put_edge(&manual_edge(
                kai_fact.id,
                scout_fact.id,
                Relation::new("contradicts").unwrap(),
            ))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine
            .generate_with_scope(
                "kai",
                super::super::BriefingScope::Unified(vec![
                    "kai".to_string(),
                    "scout".to_string(),
                ]),
            )
            .unwrap();

        let contradictions = briefing
            .sections
            .iter()
            .find(|s| s.title == "Cross-agent contradictions");

        assert!(
            contradictions.is_some(),
            "Unified briefing should surface cross-agent contradictions"
        );
        assert!(
            !contradictions.unwrap().nodes.is_empty(),
            "Contradictions section should not be empty"
        );
    }
}
