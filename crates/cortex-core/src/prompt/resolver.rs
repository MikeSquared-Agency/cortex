use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::error::{CortexError, Result};
use crate::kinds::defaults::prompt as prompt_kind;
use crate::relations::defaults::{branched_from, inherits_from, supersedes, used_by};
use crate::storage::{NodeFilter, Storage};
use crate::types::{Edge, EdgeProvenance, Node, NodeId, Source};

use super::model::{PromptContent, PromptInfo, PromptVersionInfo, ResolvedPrompt};

pub struct PromptResolver<S: Storage> {
    storage: Arc<S>,
}

// ── PromptResolver ────────────────────────────────────────────────────────────

impl<S: Storage> PromptResolver<S> {
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }

    /// Build the set of node IDs that are superseded within `nodes`.
    ///
    /// Uses *outgoing* `supersedes` edges (one `edges_from` call per node) rather
    /// than incoming edges, giving the same O(N) cost but in a single predictable
    /// forward pass. The resulting set is used for O(1) HEAD detection.
    pub fn build_superseded_set(&self, nodes: &[Node]) -> Result<HashSet<NodeId>> {
        let mut superseded = HashSet::new();
        for node in nodes {
            for edge in self.storage.edges_from(node.id)? {
                if edge.relation == supersedes() {
                    superseded.insert(edge.to);
                }
            }
        }
        Ok(superseded)
    }

    /// Return all prompt nodes with the given slug, optionally filtered to a branch.
    /// Results are sorted by created_at ascending (oldest first).
    pub fn find_versions(&self, slug: &str, branch: Option<&str>) -> Result<Vec<Node>> {
        let all = self
            .storage
            .list_nodes(NodeFilter::new().with_kinds(vec![prompt_kind()]))?;

        let mut matches: Vec<Node> = all
            .into_iter()
            .filter(|n| {
                let Ok(content) = serde_json::from_str::<PromptContent>(&n.data.body) else {
                    return false;
                };
                if content.slug != slug {
                    return false;
                }
                match branch {
                    Some(b) => content.branch == b,
                    None => true,
                }
            })
            .collect();

        matches.sort_by_key(|n| n.created_at);
        Ok(matches)
    }

    /// Return the HEAD node for slug+branch.
    ///
    /// HEAD is the version that no other sibling version has superseded.
    /// Uses `build_superseded_set` (forward-edge pass) rather than per-node
    /// reverse lookups.
    pub fn find_head(&self, slug: &str, branch: &str) -> Result<Option<Node>> {
        let versions = self.find_versions(slug, Some(branch))?;
        if versions.is_empty() {
            return Ok(None);
        }

        let superseded = self.build_superseded_set(&versions)?;
        Ok(versions.into_iter().find(|n| !superseded.contains(&n.id)))
    }

    /// Fully resolve a prompt: merge inherited sections and find associated skills.
    pub fn resolve(&self, head_node: &Node) -> Result<ResolvedPrompt> {
        let raw_content = self.parse_content(head_node)?;

        // Walk the inherits_from chain upward (max 10 hops, cycle-guarded).
        let mut ancestor_nodes: Vec<Node> = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        visited.insert(head_node.id);
        let mut current_id = head_node.id;

        for _ in 0..10 {
            let outgoing = self.storage.edges_from(current_id)?;
            let inherit_edge = outgoing.iter().find(|e| e.relation == inherits_from());
            match inherit_edge {
                None => break,
                Some(edge) => {
                    let parent_id = edge.to;
                    if !visited.insert(parent_id) {
                        break; // cycle guard: insert returns false when already present
                    }
                    let parent = self
                        .storage
                        .get_node(parent_id)?
                        .ok_or_else(|| {
                            CortexError::Validation(format!(
                                "Inherited prompt node {} not found",
                                parent_id
                            ))
                        })?;
                    ancestor_nodes.push(parent);
                    current_id = parent_id;
                }
            }
        }

        // ancestor_nodes is [parent, grandparent, ...]; reverse to get [root, ..., parent].
        ancestor_nodes.reverse();

        // Build lineage slugs: deepest ancestor first, this slug last.
        let lineage: Vec<String> = ancestor_nodes
            .iter()
            .filter_map(|n| serde_json::from_str::<PromptContent>(&n.data.body).ok())
            .map(|c| c.slug)
            .chain(std::iter::once(raw_content.slug.clone()))
            .collect();

        // Merge sections: root ancestor sets the base, each descendant overrides.
        let mut merged_sections: HashMap<String, serde_json::Value> = HashMap::new();
        for ancestor in &ancestor_nodes {
            if let Ok(content) = self.parse_content(ancestor) {
                merged_sections.extend(content.sections);
                merged_sections.extend(content.override_sections);
            }
        }
        // Head sections and overrides win unconditionally.
        merged_sections.extend(raw_content.sections.clone());
        merged_sections.extend(raw_content.override_sections.clone());

        // Find skills: nodes with a used_by edge pointing INTO this prompt.
        let skills: Vec<String> = self
            .storage
            .edges_to(head_node.id)?
            .into_iter()
            .filter(|e| e.relation == used_by())
            .filter_map(|e| {
                self.storage
                    .get_node(e.from)
                    .ok()
                    .flatten()
                    .and_then(|n| serde_json::from_str::<PromptContent>(&n.data.body).ok())
                    .map(|c| c.slug)
            })
            .collect();

        let version = raw_content.version;
        let branch = raw_content.branch.clone();

        Ok(ResolvedPrompt {
            slug: raw_content.slug.clone(),
            prompt_type: raw_content.prompt_type.clone(),
            version,
            branch,
            content: merged_sections,
            raw_content,
            lineage,
            skills,
            node_id: head_node.id,
            created_at: head_node.created_at,
        })
    }

    /// Parse PromptContent from a node's body JSON.
    pub fn parse_content(&self, node: &Node) -> Result<PromptContent> {
        serde_json::from_str(&node.data.body).map_err(|e| {
            CortexError::Validation(format!(
                "Failed to parse prompt content for node {}: {}",
                node.id, e
            ))
        })
    }

    /// Create the first version of a new prompt.
    pub fn create_prompt(&self, content: PromptContent, branch: &str, author: &str) -> Result<NodeId> {
        if !self.find_versions(&content.slug, Some(branch))?.is_empty() {
            return Err(CortexError::Validation(format!(
                "Prompt '{}' on branch '{}' already exists. Use create_version to add a new version.",
                content.slug, branch
            )));
        }

        let node = self.build_node(&content, branch, 1, author)?;
        self.storage.put_node(&node)?;
        Ok(node.id)
    }

    /// Create a new version of an existing prompt.
    pub fn create_version(
        &self,
        slug: &str,
        branch: &str,
        content: PromptContent,
        author: &str,
    ) -> Result<NodeId> {
        let head = self.find_head(slug, branch)?.ok_or_else(|| {
            CortexError::Validation(format!(
                "Prompt '{}' on branch '{}' not found",
                slug, branch
            ))
        })?;

        let head_version = self
            .parse_content(&head)
            .map(|c| c.version)
            .unwrap_or(1);

        let mut fixed_content = content;
        fixed_content.slug = slug.to_string();

        let new_node = self.build_node(&fixed_content, branch, head_version + 1, author)?;
        self.storage.put_node(&new_node)?;

        let edge = Edge::new(
            new_node.id,
            head.id,
            supersedes(),
            1.0,
            EdgeProvenance::Manual {
                created_by: author.to_string(),
            },
        );
        self.storage.put_edge(&edge)?;

        Ok(new_node.id)
    }

    /// Fork a prompt onto a new branch.
    pub fn create_branch(
        &self,
        slug: &str,
        from_branch: &str,
        new_branch: &str,
        base_version: Option<u32>,
        author: &str,
    ) -> Result<NodeId> {
        let base_node = match base_version {
            Some(v) => self.get_version(slug, from_branch, v)?.ok_or_else(|| {
                CortexError::Validation(format!(
                    "Version {} of '{}@{}' not found",
                    v, slug, from_branch
                ))
            })?,
            None => self.find_head(slug, from_branch)?.ok_or_else(|| {
                CortexError::Validation(format!(
                    "Prompt '{}' on branch '{}' not found",
                    slug, from_branch
                ))
            })?,
        };

        let base_content = self.parse_content(&base_node)?;
        let new_node = self.build_node(&base_content, new_branch, 1, author)?;
        self.storage.put_node(&new_node)?;

        let edge = Edge::new(
            new_node.id,
            base_node.id,
            branched_from(),
            1.0,
            EdgeProvenance::Manual {
                created_by: author.to_string(),
            },
        );
        self.storage.put_edge(&edge)?;

        Ok(new_node.id)
    }

    /// List the HEAD of every slug+branch combination.
    ///
    /// **Optimised**: builds the superseded set in a single forward-edge pass over
    /// all prompt nodes, then resolves each group's HEAD in O(1). This avoids the
    /// O(N × out_degree) reverse-edge lookups that the naïve per-group approach
    /// would require.
    pub fn list_all_prompts(&self) -> Result<Vec<PromptInfo>> {
        let all = self
            .storage
            .list_nodes(NodeFilter::new().with_kinds(vec![prompt_kind()]))?;

        // One forward pass: find every superseded node ID across all groups.
        let superseded = self.build_superseded_set(&all)?;

        // Keep only HEAD nodes and map them to PromptInfo.
        let mut result: Vec<PromptInfo> = all
            .iter()
            .filter(|n| !superseded.contains(&n.id))
            .filter_map(|node| {
                let content = serde_json::from_str::<PromptContent>(&node.data.body).ok()?;
                Some(PromptInfo {
                    slug: content.slug,
                    prompt_type: content.prompt_type,
                    branch: content.branch,
                    version: content.version,
                    tags: node.data.tags.clone(),
                    node_id: node.id,
                })
            })
            .collect();

        result.sort_by(|a, b| a.slug.cmp(&b.slug).then(a.branch.cmp(&b.branch)));
        Ok(result)
    }

    /// Get a specific version of a prompt by version number.
    pub fn get_version(&self, slug: &str, branch: &str, version_num: u32) -> Result<Option<Node>> {
        let versions = self.find_versions(slug, Some(branch))?;
        Ok(versions.into_iter().find(|n| {
            serde_json::from_str::<PromptContent>(&n.data.body)
                .map(|c| c.version == version_num)
                .unwrap_or(false)
        }))
    }

    /// List version history for a slug+branch, optimised with a single
    /// superseded-set pass (avoids per-node reverse-edge lookups).
    pub fn list_versions(&self, slug: &str, branch: &str) -> Result<Vec<PromptVersionInfo>> {
        let versions = self.find_versions(slug, Some(branch))?;
        let superseded = self.build_superseded_set(&versions)?;

        let infos = versions
            .iter()
            .map(|node| {
                let version = serde_json::from_str::<PromptContent>(&node.data.body)
                    .map(|c| c.version)
                    .unwrap_or(1);
                PromptVersionInfo {
                    node_id: node.id,
                    slug: slug.to_string(),
                    version,
                    branch: branch.to_string(),
                    created_at: node.created_at,
                    is_head: !superseded.contains(&node.id),
                }
            })
            .collect();

        Ok(infos)
    }

    /// Build a Node for a given PromptContent (does not store it).
    /// The `branch` and `version` parameters are embedded into the serialised
    /// body so they can be read back later without touching node metadata
    /// (which uses bincode and cannot round-trip `serde_json::Value`).
    fn build_node(&self, content: &PromptContent, branch: &str, version: u32, author: &str) -> Result<Node> {
        let mut full_content = content.clone();
        full_content.branch = branch.to_string();
        full_content.version = version;

        let body = serde_json::to_string(&full_content)
            .map_err(|e| CortexError::Validation(format!("Failed to serialize prompt: {}", e)))?;

        let title = format!("{}@{}/v{}", content.slug, branch, version);

        let node = Node::new(
            prompt_kind(),
            title,
            body,
            Source {
                agent: author.to_string(),
                session: None,
                channel: None,
            },
            0.7,
        );

        Ok(node)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use std::collections::HashMap;
    use tempfile::TempDir;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn setup() -> (Arc<RedbStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
        (storage, dir)
    }

    fn simple_content(slug: &str, prompt_type: &str, sections: &[(&str, &str)]) -> PromptContent {
        PromptContent {
            slug: slug.to_owned(),
            prompt_type: prompt_type.to_owned(),
            branch: "main".to_owned(), // overridden by build_node
            version: 1,                // overridden by build_node
            sections: sections
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
                .collect(),
            metadata: HashMap::new(),
            override_sections: HashMap::new(),
        }
    }

    // ── create_prompt ─────────────────────────────────────────────────────────

    #[test]
    fn create_prompt_stores_v1() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());
        let content = simple_content("kai-soul", "persona", &[("identity", "I am Kai")]);
        let id = r.create_prompt(content, "main", "test").unwrap();

        let node = storage.get_node(id).unwrap().unwrap();
        let parsed = r.parse_content(&node).unwrap();
        assert_eq!(parsed.slug, "kai-soul");
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.prompt_type, "persona");
        assert_eq!(node.data.title, "kai-soul@main/v1");
    }

    #[test]
    fn create_prompt_duplicate_fails() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        let content = simple_content("slug", "persona", &[]);
        r.create_prompt(content.clone(), "main", "test").unwrap();
        let err = r.create_prompt(content, "main", "test").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn create_prompt_different_branches_independent() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        let c = || simple_content("slug", "persona", &[]);
        r.create_prompt(c(), "main", "test").unwrap();
        r.create_prompt(c(), "dev", "test").unwrap(); // must not fail
    }

    // ── find_head + version chain ─────────────────────────────────────────────

    #[test]
    fn find_head_single_version() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        let id = r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        let head = r.find_head("p", "main").unwrap().unwrap();
        assert_eq!(head.id, id);
    }

    #[test]
    fn find_head_returns_newest_in_chain() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("p", "persona", &[("k", "v1")]), "main", "t").unwrap();
        let v2 = r.create_version("p", "main", simple_content("p", "persona", &[("k", "v2")]), "t").unwrap();
        let v3 = r.create_version("p", "main", simple_content("p", "persona", &[("k", "v3")]), "t").unwrap();

        let head = r.find_head("p", "main").unwrap().unwrap();
        assert_eq!(head.id, v3, "HEAD should be v3, the latest");
        let _ = v2; // v2 is a historical version, not HEAD
    }

    #[test]
    fn find_head_missing_slug_returns_none() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        assert!(r.find_head("nonexistent", "main").unwrap().is_none());
    }

    #[test]
    fn find_head_missing_branch_returns_none() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        assert!(r.find_head("p", "dev").unwrap().is_none());
    }

    // ── create_version ────────────────────────────────────────────────────────

    #[test]
    fn create_version_increments_number() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        let v2_id = r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();

        let v2 = storage.get_node(v2_id).unwrap().unwrap();
        let parsed = r.parse_content(&v2).unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(v2.data.title, "p@main/v2");
    }

    #[test]
    fn create_version_creates_supersedes_edge() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        let v1_id = r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        let v2_id = r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();

        let edges = storage.edges_from(v2_id).unwrap();
        assert!(
            edges.iter().any(|e| e.relation == supersedes() && e.to == v1_id),
            "v2 must have a supersedes edge pointing to v1"
        );
    }

    #[test]
    fn create_version_on_missing_slug_fails() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        let err = r
            .create_version("missing", "main", simple_content("missing", "persona", &[]), "t")
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    // ── create_branch ─────────────────────────────────────────────────────────

    #[test]
    fn create_branch_copies_head_content() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        r.create_prompt(simple_content("p", "persona", &[("id", "base")]), "main", "t").unwrap();
        let dev_id = r.create_branch("p", "main", "dev", None, "t").unwrap();

        let dev_node = storage.get_node(dev_id).unwrap().unwrap();
        let parsed = r.parse_content(&dev_node).unwrap();
        assert_eq!(parsed.branch, "dev");
        assert_eq!(parsed.version, 1);
    }

    #[test]
    fn create_branch_creates_branched_from_edge() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        let main_id = r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        let dev_id = r.create_branch("p", "main", "dev", None, "t").unwrap();

        let edges = storage.edges_from(dev_id).unwrap();
        assert!(
            edges.iter().any(|e| e.relation == branched_from() && e.to == main_id),
            "dev branch must have branched_from edge to main HEAD"
        );
    }

    #[test]
    fn create_branch_from_specific_version() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        let v1_id = r.create_prompt(simple_content("p", "persona", &[("k", "v1")]), "main", "t").unwrap();
        let _v2_id = r.create_version("p", "main", simple_content("p", "persona", &[("k", "v2")]), "t").unwrap();

        // Branch from v1 specifically, not HEAD (v2)
        let branch_id = r.create_branch("p", "main", "hotfix", Some(1), "t").unwrap();

        let edges = storage.edges_from(branch_id).unwrap();
        assert!(
            edges.iter().any(|e| e.relation == branched_from() && e.to == v1_id),
            "hotfix must branch from v1, not v2"
        );
    }

    // ── list_all_prompts ──────────────────────────────────────────────────────

    #[test]
    fn list_all_prompts_returns_heads_only() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("a", "persona", &[]), "main", "t").unwrap();
        r.create_version("a", "main", simple_content("a", "persona", &[("k", "v2")]), "t").unwrap();
        let v3 = r.create_version("a", "main", simple_content("a", "persona", &[("k", "v3")]), "t").unwrap();

        let list = r.list_all_prompts().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].slug, "a");
        assert_eq!(list[0].version, 3);
        assert_eq!(list[0].node_id, v3);
    }

    #[test]
    fn list_all_prompts_multiple_slugs_and_branches() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("kai", "persona", &[]), "main", "t").unwrap();
        r.create_prompt(simple_content("kai", "persona", &[]), "dev", "t").unwrap();
        r.create_prompt(simple_content("duty", "persona", &[]), "main", "t").unwrap();

        let list = r.list_all_prompts().unwrap();
        assert_eq!(list.len(), 3);

        // Should be sorted by slug then branch.
        assert_eq!(list[0].slug, "duty");
        assert_eq!(list[1].slug, "kai");
        assert_eq!(list[1].branch, "dev");
        assert_eq!(list[2].slug, "kai");
        assert_eq!(list[2].branch, "main");
    }

    #[test]
    fn list_all_prompts_empty_db() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        assert!(r.list_all_prompts().unwrap().is_empty());
    }

    // ── list_versions ─────────────────────────────────────────────────────────

    #[test]
    fn list_versions_marks_only_head() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();
        r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();

        let versions = r.list_versions("p", "main").unwrap();
        assert_eq!(versions.len(), 3);

        let heads: Vec<_> = versions.iter().filter(|v| v.is_head).collect();
        assert_eq!(heads.len(), 1, "exactly one version should be HEAD");
        assert_eq!(heads[0].version, 3, "HEAD must be v3");

        let non_heads: Vec<_> = versions.iter().filter(|v| !v.is_head).collect();
        assert_eq!(non_heads.len(), 2);
    }

    #[test]
    fn list_versions_empty_for_missing_slug() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);
        assert!(r.list_versions("missing", "main").unwrap().is_empty());
    }

    #[test]
    fn list_versions_sorted_ascending() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();
        r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();

        let versions = r.list_versions("p", "main").unwrap();
        let nums: Vec<u32> = versions.iter().map(|v| v.version).collect();
        assert_eq!(nums, vec![1, 2, 3]);
    }

    // ── get_version ───────────────────────────────────────────────────────────

    #[test]
    fn get_version_returns_correct_node() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        let v1 = r.create_prompt(simple_content("p", "persona", &[("k", "v1")]), "main", "t").unwrap();
        let v2 = r.create_version("p", "main", simple_content("p", "persona", &[("k", "v2")]), "t").unwrap();

        assert_eq!(r.get_version("p", "main", 1).unwrap().unwrap().id, v1);
        assert_eq!(r.get_version("p", "main", 2).unwrap().unwrap().id, v2);
        assert!(r.get_version("p", "main", 99).unwrap().is_none());
    }

    // ── resolve (inheritance) ─────────────────────────────────────────────────

    fn link_inherits(storage: &Arc<RedbStorage>, child_id: NodeId, parent_id: NodeId) {
        let edge = Edge::new(
            child_id,
            parent_id,
            inherits_from(),
            1.0,
            EdgeProvenance::Manual { created_by: "test".into() },
        );
        storage.put_edge(&edge).unwrap();
    }

    #[test]
    fn resolve_simple_no_inheritance() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        let id = r
            .create_prompt(simple_content("p", "persona", &[("role", "assistant")]), "main", "t")
            .unwrap();
        let node = r.find_head("p", "main").unwrap().unwrap();
        let resolved = r.resolve(&node).unwrap();

        assert_eq!(resolved.slug, "p");
        assert_eq!(resolved.version, 1);
        assert!(resolved.lineage == vec!["p".to_string()]);
        assert_eq!(
            resolved.content.get("role").and_then(|v| v.as_str()),
            Some("assistant")
        );
        assert!(resolved.skills.is_empty());
        let _ = id;
    }

    #[test]
    fn resolve_single_level_inheritance() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        // Parent: base-template with two sections
        r.create_prompt(
            simple_content("base", "template", &[("tone", "formal"), ("language", "en")]),
            "main",
            "t",
        )
        .unwrap();
        // Child overrides "tone" but inherits "language"
        r.create_prompt(
            simple_content("kai", "persona", &[("tone", "friendly")]),
            "main",
            "t",
        )
        .unwrap();

        let base_node = r.find_head("base", "main").unwrap().unwrap();
        let kai_node = r.find_head("kai", "main").unwrap().unwrap();
        link_inherits(&storage, kai_node.id, base_node.id);

        let resolved = r.resolve(&kai_node).unwrap();

        // Language comes from base; tone overridden by kai.
        assert_eq!(resolved.content["language"].as_str(), Some("en"), "inherited from base");
        assert_eq!(resolved.content["tone"].as_str(), Some("friendly"), "overridden by kai");
        assert_eq!(resolved.lineage, vec!["base", "kai"]);
    }

    #[test]
    fn resolve_deep_inheritance_chain() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        r.create_prompt(simple_content("root", "template", &[("a", "root-a"), ("b", "root-b")]), "main", "t").unwrap();
        r.create_prompt(simple_content("mid", "template", &[("b", "mid-b"), ("c", "mid-c")]), "main", "t").unwrap();
        r.create_prompt(simple_content("leaf", "persona", &[("c", "leaf-c"), ("d", "leaf-d")]), "main", "t").unwrap();

        let root = r.find_head("root", "main").unwrap().unwrap();
        let mid = r.find_head("mid", "main").unwrap().unwrap();
        let leaf = r.find_head("leaf", "main").unwrap().unwrap();
        link_inherits(&storage, mid.id, root.id);
        link_inherits(&storage, leaf.id, mid.id);

        let resolved = r.resolve(&leaf).unwrap();

        assert_eq!(resolved.content["a"].as_str(), Some("root-a"), "a from root");
        assert_eq!(resolved.content["b"].as_str(), Some("mid-b"), "b overridden by mid");
        assert_eq!(resolved.content["c"].as_str(), Some("leaf-c"), "c overridden by leaf");
        assert_eq!(resolved.content["d"].as_str(), Some("leaf-d"), "d only in leaf");
        assert_eq!(resolved.lineage, vec!["root", "mid", "leaf"]);
    }

    #[test]
    fn resolve_cycle_guard() {
        // Deliberately create a cycle: A → B → A (inherits_from)
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage.clone());

        r.create_prompt(simple_content("a", "persona", &[("k", "va")]), "main", "t").unwrap();
        r.create_prompt(simple_content("b", "persona", &[("k", "vb")]), "main", "t").unwrap();

        let a = r.find_head("a", "main").unwrap().unwrap();
        let b = r.find_head("b", "main").unwrap().unwrap();
        link_inherits(&storage, a.id, b.id);
        link_inherits(&storage, b.id, a.id); // creates cycle

        // Must not panic or loop forever; should return without error.
        let resolved = r.resolve(&a).unwrap();
        // "k" should exist — from whichever side it stopped the cycle
        assert!(resolved.content.contains_key("k"));
    }

    // ── parse_content ─────────────────────────────────────────────────────────

    #[test]
    fn parse_content_round_trips() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        let original = PromptContent {
            slug: "kai".into(),
            prompt_type: "persona".into(),
            branch: "main".into(),
            version: 1,
            sections: [("role".to_string(), serde_json::Value::String("assistant".into()))]
                .into_iter()
                .collect(),
            metadata: HashMap::new(),
            override_sections: HashMap::new(),
        };

        let id = r.create_prompt(original.clone(), "main", "t").unwrap();
        let node = r.find_head("kai", "main").unwrap().unwrap();
        let parsed = r.parse_content(&node).unwrap();

        assert_eq!(parsed.slug, original.slug);
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.prompt_type, original.prompt_type);
        assert_eq!(parsed.sections, original.sections);
        let _ = id;
    }

    // ── build_superseded_set ──────────────────────────────────────────────────

    #[test]
    fn superseded_set_empty_for_single_version() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        let versions = r.find_versions("p", Some("main")).unwrap();
        let superseded = r.build_superseded_set(&versions).unwrap();

        assert!(superseded.is_empty(), "no version has been superseded yet");
    }

    #[test]
    fn superseded_set_excludes_head() {
        let (storage, _dir) = setup();
        let r = PromptResolver::new(storage);

        r.create_prompt(simple_content("p", "persona", &[]), "main", "t").unwrap();
        r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();
        let v3_id = r.create_version("p", "main", simple_content("p", "persona", &[]), "t").unwrap();

        let versions = r.find_versions("p", Some("main")).unwrap();
        let superseded = r.build_superseded_set(&versions).unwrap();

        assert_eq!(superseded.len(), 2, "v1 and v2 are superseded");
        assert!(!superseded.contains(&v3_id), "HEAD (v3) must not be in the superseded set");
    }
}
