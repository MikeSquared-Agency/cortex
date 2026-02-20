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

impl<S: Storage> PromptResolver<S> {
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
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
                let node_slug = n
                    .data
                    .metadata
                    .get("prompt_slug")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if node_slug != slug {
                    return false;
                }
                if let Some(b) = branch {
                    let node_branch = n
                        .data
                        .metadata
                        .get("prompt_branch")
                        .and_then(|v| v.as_str())
                        .unwrap_or("main");
                    node_branch == b
                } else {
                    true
                }
            })
            .collect();

        matches.sort_by_key(|n| n.created_at);
        Ok(matches)
    }

    /// Return the HEAD node for slug+branch (the one not superseded by any sibling).
    pub fn find_head(&self, slug: &str, branch: &str) -> Result<Option<Node>> {
        let versions = self.find_versions(slug, Some(branch))?;
        if versions.is_empty() {
            return Ok(None);
        }

        let id_set: HashSet<NodeId> = versions.iter().map(|n| n.id).collect();

        for node in &versions {
            let incoming = self.storage.edges_to(node.id)?;
            let is_superseded = incoming
                .iter()
                .any(|e| e.relation == supersedes() && id_set.contains(&e.from));
            if !is_superseded {
                return Ok(Some(node.clone()));
            }
        }

        // Fallback: return the last (should not normally occur in a valid chain)
        Ok(versions.into_iter().last())
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
                    if visited.contains(&parent_id) {
                        break; // cycle guard
                    }
                    visited.insert(parent_id);
                    let parent = self
                        .storage
                        .get_node(parent_id)?
                        .ok_or_else(|| {
                            CortexError::Validation(format!(
                                "Inherited prompt node {} not found",
                                parent_id
                            ))
                        })?;
                    ancestor_nodes.push(parent.clone());
                    current_id = parent_id;
                }
            }
        }

        // ancestor_nodes is [parent, grandparent, ...]; reverse to get [root, ..., parent].
        ancestor_nodes.reverse();

        // Build lineage slugs: deepest ancestor first, this slug last.
        let lineage: Vec<String> = ancestor_nodes
            .iter()
            .filter_map(|n| {
                n.data
                    .metadata
                    .get("prompt_slug")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .chain(std::iter::once(raw_content.slug.clone()))
            .collect();

        // Merge sections: root ancestor sets the base, each descendant overrides.
        let mut merged_sections: HashMap<String, serde_json::Value> = HashMap::new();
        for ancestor in &ancestor_nodes {
            if let Ok(content) = self.parse_content(ancestor) {
                for (k, v) in content.sections {
                    merged_sections.insert(k, v);
                }
                for (k, v) in content.override_sections {
                    merged_sections.insert(k, v);
                }
            }
        }
        // Apply head's sections and overrides last.
        for (k, v) in &raw_content.sections {
            merged_sections.insert(k.clone(), v.clone());
        }
        for (k, v) in &raw_content.override_sections {
            merged_sections.insert(k.clone(), v.clone());
        }

        // Find skills: nodes with a used_by edge pointing INTO this prompt.
        let incoming = self.storage.edges_to(head_node.id)?;
        let skills: Vec<String> = incoming
            .iter()
            .filter(|e| e.relation == used_by())
            .filter_map(|e| {
                self.storage
                    .get_node(e.from)
                    .ok()
                    .flatten()
                    .and_then(|n| {
                        n.data
                            .metadata
                            .get("prompt_slug")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
            })
            .collect();

        let version = head_node
            .data
            .metadata
            .get("prompt_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let branch = head_node
            .data
            .metadata
            .get("prompt_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string();

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
        let existing = self.find_versions(&content.slug, Some(branch))?;
        if !existing.is_empty() {
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

        let head_version = head
            .data
            .metadata
            .get("prompt_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let mut fixed_content = content;
        fixed_content.slug = slug.to_string();

        let new_node = self.build_node(&fixed_content, branch, head_version + 1, author)?;
        self.storage.put_node(&new_node)?;

        // supersedes edge: new_node → old head
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
            Some(v) => self
                .get_version(slug, from_branch, v)?
                .ok_or_else(|| {
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

        // branched_from edge: new_node → base_node
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
    pub fn list_all_prompts(&self) -> Result<Vec<PromptInfo>> {
        let all = self
            .storage
            .list_nodes(NodeFilter::new().with_kinds(vec![prompt_kind()]))?;

        // Group nodes by (slug, branch).
        let mut groups: HashMap<(String, String), Vec<Node>> = HashMap::new();
        for node in all {
            let slug = node
                .data
                .metadata
                .get("prompt_slug")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let branch = node
                .data
                .metadata
                .get("prompt_branch")
                .and_then(|v| v.as_str())
                .unwrap_or("main")
                .to_string();
            if slug.is_empty() {
                continue;
            }
            groups.entry((slug, branch)).or_default().push(node);
        }

        let mut result = Vec::new();
        for ((slug, branch), mut nodes) in groups {
            nodes.sort_by_key(|n| n.created_at);
            let id_set: HashSet<NodeId> = nodes.iter().map(|n| n.id).collect();

            let head = nodes.iter().find(|node| {
                self.storage
                    .edges_to(node.id)
                    .unwrap_or_default()
                    .iter()
                    .all(|e| !(e.relation == supersedes() && id_set.contains(&e.from)))
            });

            if let Some(node) = head {
                let prompt_type = node
                    .data
                    .metadata
                    .get("prompt_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let version = node
                    .data
                    .metadata
                    .get("prompt_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;

                result.push(PromptInfo {
                    slug,
                    prompt_type,
                    branch,
                    version,
                    tags: node.data.tags.clone(),
                    node_id: node.id,
                });
            }
        }

        result.sort_by(|a, b| a.slug.cmp(&b.slug).then(a.branch.cmp(&b.branch)));
        Ok(result)
    }

    /// Get a specific version of a prompt by version number.
    pub fn get_version(&self, slug: &str, branch: &str, version_num: u32) -> Result<Option<Node>> {
        let versions = self.find_versions(slug, Some(branch))?;
        Ok(versions.into_iter().find(|n| {
            n.data
                .metadata
                .get("prompt_version")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                == Some(version_num)
        }))
    }

    /// List version history for a slug+branch.
    pub fn list_versions(&self, slug: &str, branch: &str) -> Result<Vec<PromptVersionInfo>> {
        let versions = self.find_versions(slug, Some(branch))?;
        let id_set: HashSet<NodeId> = versions.iter().map(|n| n.id).collect();

        let infos = versions
            .iter()
            .map(|node| {
                let is_head = self
                    .storage
                    .edges_to(node.id)
                    .unwrap_or_default()
                    .iter()
                    .all(|e| !(e.relation == supersedes() && id_set.contains(&e.from)));

                let version = node
                    .data
                    .metadata
                    .get("prompt_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;

                PromptVersionInfo {
                    node_id: node.id,
                    slug: slug.to_string(),
                    version,
                    branch: branch.to_string(),
                    created_at: node.created_at,
                    is_head,
                }
            })
            .collect();

        Ok(infos)
    }

    /// Build a Node for a given PromptContent (does not store it).
    fn build_node(&self, content: &PromptContent, branch: &str, version: u32, author: &str) -> Result<Node> {
        let body = serde_json::to_string(content)
            .map_err(|e| CortexError::Validation(format!("Failed to serialize prompt: {}", e)))?;

        let title = format!("{}@{}/v{}", content.slug, branch, version);

        let mut node = Node::new(
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

        node.data.metadata.insert(
            "prompt_slug".to_string(),
            serde_json::Value::String(content.slug.clone()),
        );
        node.data.metadata.insert(
            "prompt_branch".to_string(),
            serde_json::Value::String(branch.to_string()),
        );
        node.data.metadata.insert(
            "prompt_type".to_string(),
            serde_json::Value::String(content.prompt_type.clone()),
        );
        node.data.metadata.insert(
            "prompt_version".to_string(),
            serde_json::Value::Number(version.into()),
        );

        Ok(node)
    }
}
