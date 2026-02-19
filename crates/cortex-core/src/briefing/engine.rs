use super::{Briefing, BriefingSection};
use super::cache::BriefingCache;
use super::renderer::{BriefingRenderer, CompactRenderer, MarkdownRenderer};
use crate::error::Result;
use crate::graph::{GraphEngine, TraversalDirection, TraversalRequest};
use crate::storage::{NodeFilter, Storage};
use crate::types::{Node, NodeId, NodeKind, Relation};
use crate::vector::{EmbeddingService, HybridQuery, HybridSearch, VectorIndex};
use chrono::Utc;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
            // 2. Patterns (via graph traversal)
            let patterns = self.generate_patterns(aid, &seen_ids)?;
            if !patterns.nodes.is_empty() {
                for n in &patterns.nodes {
                    seen_ids.insert(n.id);
                }
                sections.push(patterns);
            }

            // 3. Goals (via graph traversal)
            let goals = self.generate_goals(aid, &seen_ids)?;
            if !goals.nodes.is_empty() {
                for n in &goals.nodes {
                    seen_ids.insert(n.id);
                }
                sections.push(goals);
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
            // No agent node — fall back to global queries by kind
            let global_patterns = self.generate_global_by_kind("pattern", "Patterns", &seen_ids)?;
            if !global_patterns.nodes.is_empty() {
                for n in &global_patterns.nodes { seen_ids.insert(n.id); }
                sections.push(global_patterns);
            }

            let global_goals = self.generate_global_by_kind("goal", "Goals", &seen_ids)?;
            if !global_goals.nodes.is_empty() {
                for n in &global_goals.nodes { seen_ids.insert(n.id); }
                sections.push(global_goals);
            }

            let global_decisions = self.generate_global_by_kind("decision", "Key Decisions", &seen_ids)?;
            if !global_decisions.nodes.is_empty() {
                for n in &global_decisions.nodes { seen_ids.insert(n.id); }
                sections.push(global_decisions);
            }
        }

        // 5. Active Context (fills remaining slots with recent nodes not already covered)
        let active = self.generate_active_context(agent_id, agent_node_id, &seen_ids)?;
        if !active.nodes.is_empty() {
            for n in &active.nodes {
                seen_ids.insert(n.id);
            }
            sections.push(active);
        }

        // 6. Recent Events
        let events = self.generate_recent_events(agent_id, &seen_ids)?;
        if !events.nodes.is_empty() {
            sections.push(events);
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

    /// Filter nodes below `min_importance` and sort by importance desc,
    /// access_count desc. Applied uniformly across all section generators.
    fn rank(&self, mut nodes: Vec<Node>) -> Vec<Node> {
        nodes.retain(|n| n.importance >= self.config.min_importance);
        nodes.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.access_count.cmp(&a.access_count))
        });
        nodes
    }

    // --- Private section generators ---

    fn find_agent_node(&self, agent_id: &str) -> Result<Option<NodeId>> {
        // Primary: Agent node whose source_agent matches
        let nodes = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(vec![NodeKind::new("agent").unwrap()])
                .with_source_agent(agent_id.to_string())
                .with_limit(1),
        )?;

        if let Some(n) = nodes.first() {
            return Ok(Some(n.id));
        }

        // Fallback: search by tag (agents should be tagged with their ID)
        let by_tag = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(vec![NodeKind::new("agent").unwrap()])
                .with_tags(vec![agent_id.to_lowercase()])
                .with_limit(1),
        )?;
        if let Some(n) = by_tag.first() {
            return Ok(Some(n.id));
        }

        // Last resort: scan Agent nodes for title/source match
        let all_agents = self.storage.list_nodes(
            NodeFilter::new()
                .with_kinds(vec![NodeKind::new("agent").unwrap()])
                .with_limit(50),
        )?;

        for node in &all_agents {
            if node.data.title.to_lowercase().contains(&agent_id.to_lowercase())
                || node.source.agent == agent_id
            {
                return Ok(Some(node.id));
            }
        }

        Ok(None)
    }

    fn generate_identity(
        &self,
        agent_id: &str,
        agent_node_id: Option<NodeId>,
    ) -> Result<BriefingSection> {
        let mut nodes: Vec<Node> = Vec::new();

        if let Some(aid) = agent_node_id {
            // Include the Agent node itself (always, regardless of importance)
            if let Ok(Some(agent_node)) = self.storage.get_node(aid) {
                nodes.push(agent_node);
            }

            // Preferences/Facts connected via AppliesTo (either direction)
            let neighbors =
                self.graph
                    .neighbors(aid, TraversalDirection::Both, Some(vec![Relation::new("applies_to").unwrap()]))?;

            let pref_nodes: Vec<Node> = neighbors
                .into_iter()
                .filter_map(|(node, _edge)| {
                    if matches!(node.kind.as_str(), "preference" | "fact") {
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
            let fallback = self.storage.list_nodes(
                NodeFilter::new()
                    .with_source_agent(agent_id.to_string())
                    .with_kinds(vec![NodeKind::new("agent").unwrap(), NodeKind::new("preference").unwrap()])
                    .with_min_importance(self.config.min_importance)
                    .with_limit(self.config.max_items_per_section * 2),
            )?;
            nodes.extend(self.rank(fallback));
        }

        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Identity & Preferences".to_string(),
            nodes,
        })
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
            by_importance
                .sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap_or(std::cmp::Ordering::Equal));
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

    fn generate_patterns(
        &self,
        agent_node_id: NodeId,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let result = self.graph.traverse(TraversalRequest {
            start: vec![agent_node_id],
            max_depth: Some(2),
            direction: TraversalDirection::Both,
            relation_filter: Some(vec![Relation::new("applies_to").unwrap(), Relation::new("instance_of").unwrap()]),
            kind_filter: Some(vec![NodeKind::new("pattern").unwrap()]),
            ..Default::default()
        })?;

        let candidates: Vec<Node> = result
            .nodes
            .into_values()
            .filter(|n| n.id != agent_node_id && !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Patterns".to_string(),
            nodes,
        })
    }

    fn generate_goals(
        &self,
        agent_node_id: NodeId,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let result = self.graph.traverse(TraversalRequest {
            start: vec![agent_node_id],
            max_depth: Some(2),
            direction: TraversalDirection::Both,
            kind_filter: Some(vec![NodeKind::new("goal").unwrap()]),
            ..Default::default()
        })?;

        let candidates: Vec<Node> = result
            .nodes
            .into_values()
            .filter(|n| n.id != agent_node_id && !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Goals".to_string(),
            nodes,
        })
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
                n.id != agent_node_id
                    && !seen.contains(&n.id)
                    && contradicting_ids.contains(&n.id)
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

    fn generate_recent_events(
        &self,
        agent_id: &str,
        seen: &HashSet<NodeId>,
    ) -> Result<BriefingSection> {
        let cutoff =
            Utc::now() - chrono::Duration::seconds(self.config.recent_window.as_secs() as i64);

        // Try agent-specific events first, fall back to global
        let mut raw = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(agent_id.to_string())
                .with_kinds(vec![NodeKind::new("event").unwrap()])
                .created_after(cutoff)
                .with_limit(self.config.max_items_per_section * 2),
        )?;

        if raw.is_empty() {
            raw = self.storage.list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![NodeKind::new("event").unwrap()])
                    .created_after(cutoff)
                    .with_limit(self.config.max_items_per_section * 2),
            )?;
        }

        let candidates: Vec<Node> = raw
            .into_iter()
            .filter(|n| !seen.contains(&n.id))
            .collect();

        let mut nodes = self.rank(candidates);
        nodes.truncate(self.config.max_items_per_section);

        Ok(BriefingSection {
            title: "Recent Events".to_string(),
            nodes,
        })
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
            .put_edge(&manual_edge(pref.id, agent.id, Relation::new("applies_to").unwrap()))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Identity & Preferences")
            .expect("identity section missing");

        assert!(
            section.nodes.iter().any(|n| n.kind == NodeKind::new("preference").unwrap()),
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
        let pattern = make_node(NodeKind::new("pattern").unwrap(), "Recurring pattern", "kai");
        storage.put_node(&agent).unwrap();
        storage.put_node(&pattern).unwrap();
        storage
            .put_edge(&manual_edge(pattern.id, agent.id, Relation::new("applies_to").unwrap()))
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
        assert!(section.nodes.iter().any(|n| n.kind == NodeKind::new("pattern").unwrap()));
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
            .put_edge(&manual_edge(agent.id, fact1.id, Relation::new("informed_by").unwrap()))
            .unwrap();
        storage
            .put_edge(&manual_edge(fact1.id, fact2.id, Relation::new("contradicts").unwrap()))
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
            let pref = make_node(NodeKind::new("preference").unwrap(), &format!("Pref {}", i), "kai");
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(pref.id, agent.id, Relation::new("applies_to").unwrap()))
                .unwrap();
        }

        let config = BriefingConfig {
            max_items_per_section: 5,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine =
            BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

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
            let pref = make_node(NodeKind::new("preference").unwrap(), &format!("Pref {}", i), "kai");
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(pref.id, agent.id, Relation::new("applies_to").unwrap()))
                .unwrap();
        }

        let config = BriefingConfig {
            max_items_per_section: 20,
            max_total_items: 10,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine =
            BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

        let briefing = engine.generate("kai").unwrap();

        let total: usize = briefing.sections.iter().map(|s| s.nodes.len()).sum();
        assert!(
            total <= 10,
            "Total {} exceeds max_total_items 10",
            total
        );
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
                nodes: vec![make_node(NodeKind::new("fact").unwrap(), "A fact with a rather long title", "test")],
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
                nodes: vec![make_node(NodeKind::new("agent").unwrap(), "Kai Agent", "kai")],
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
                    .map(|i| make_node(NodeKind::new("fact").unwrap(), &format!("Long fact title number {}", i), "kai"))
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
            .put_edge(&manual_edge(agent.id, goal.id, Relation::new("informed_by").unwrap()))
            .unwrap();

        let (engine, _) = make_engine(storage);
        let briefing = engine.generate("kai").unwrap();

        let section = briefing
            .sections
            .iter()
            .find(|s| s.title == "Goals")
            .expect("Goals section missing");

        assert!(section.nodes.iter().any(|n| n.kind == NodeKind::new("goal").unwrap()));
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
            all_nodes.iter().any(|n| n.kind == NodeKind::new("event").unwrap()),
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
            let ev = make_node(NodeKind::new("event").unwrap(), &format!("Event {}", i), "kai");
            storage.put_node(&ev).unwrap();
        }

        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine =
            BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

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
            .put_edge(&manual_edge(good_pref.id, agent.id, Relation::new("applies_to").unwrap()))
            .unwrap();
        storage
            .put_edge(&manual_edge(bad_pref.id, agent.id, Relation::new("applies_to").unwrap()))
            .unwrap();

        let config = BriefingConfig {
            min_importance: 0.5,
            ..Default::default()
        };
        let graph = Arc::new(GraphEngineImpl::new(storage.clone()));
        let gv = Arc::new(AtomicU64::new(0));
        let engine =
            BriefingEngine::new(storage, graph, MockVectorIndex, MockEmbedder, gv, config);

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
            let mut pref = make_node(NodeKind::new("preference").unwrap(), &format!("Pref {}", i), "kai");
            pref.importance = importance;
            storage.put_node(&pref).unwrap();
            storage
                .put_edge(&manual_edge(pref.id, agent.id, Relation::new("applies_to").unwrap()))
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
            .put_edge(&manual_edge(pref.id, agent.id, Relation::new("applies_to").unwrap()))
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
}
