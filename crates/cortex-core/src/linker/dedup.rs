use crate::error::Result;
use crate::graph::GraphEngine;
use crate::storage::Storage;
use crate::types::{Edge, EdgeProvenance, Node, NodeId, Relation};
use crate::vector::{SimilarityConfig, VectorIndex};
use chrono::Utc;
use std::sync::{Arc, RwLock};

/// Action to take for a duplicate pair
#[derive(Debug, Clone, PartialEq)]
pub enum DedupAction {
    /// Merge B into A (A is older/more connected/higher importance).
    /// Creates Supersedes edge, tombstones B, transfers B's edges to A.
    Merge { keep: NodeId, retire: NodeId },

    /// Link with Supersedes (newer replaces older) but keep both.
    /// Used when both have unique edges worth preserving.
    Supersede { newer: NodeId, older: NodeId },

    /// They're similar but distinct. Link with RelatedTo.
    /// Used when similarity is high but content differs meaningfully.
    Link,
}

/// Pair of near-duplicate nodes
#[derive(Debug, Clone)]
pub struct DuplicatePair {
    pub node_a: NodeId,
    pub node_b: NodeId,
    pub similarity: f32,
    pub suggestion: DedupAction,
}

/// Result from deduplication scan
#[derive(Debug, Clone)]
pub struct DedupResult {
    pub duplicates: Vec<DuplicatePair>,
}

/// Scanner for detecting and handling duplicate nodes

#[allow(dead_code)]
pub struct DedupScanner<S: Storage, V: VectorIndex, G: GraphEngine> {
    storage: Arc<S>,
    vector_index: Arc<RwLock<V>>,
    graph_engine: Arc<G>,
    config: SimilarityConfig,
}

impl<S: Storage, V: VectorIndex, G: GraphEngine> DedupScanner<S, V, G> {
    pub fn new(
        storage: Arc<S>,
        vector_index: Arc<RwLock<V>>,
        graph_engine: Arc<G>,
        config: SimilarityConfig,
    ) -> Self {
        Self {
            storage,
            vector_index,
            graph_engine,
            config,
        }
    }

    /// Scan for duplicate nodes
    pub fn scan(&self) -> Result<DedupResult> {
        let mut duplicates = Vec::new();
        let mut seen_pairs = std::collections::HashSet::new();

        // Get all nodes
        let all_nodes = self.storage.list_nodes(crate::storage::NodeFilter::new())?;

        for node in &all_nodes {
            // Skip deleted nodes
            if node.deleted {
                continue;
            }

            // Skip if no embedding
            let embedding = match &node.embedding {
                Some(emb) => emb,
                None => continue,
            };

            // Find similar nodes
            let vector_index = self.vector_index.read().unwrap();
            let similar =
                vector_index.search_threshold(embedding, self.config.dedup_threshold, None)?;
            drop(vector_index);

            for result in similar {
                // Skip self
                if result.node_id == node.id {
                    continue;
                }

                // Skip if already seen this pair
                let pair_key = if node.id < result.node_id {
                    (node.id, result.node_id)
                } else {
                    (result.node_id, node.id)
                };

                if seen_pairs.contains(&pair_key) {
                    continue;
                }
                seen_pairs.insert(pair_key);

                // Get other node
                let other = match self.storage.get_node(result.node_id)? {
                    Some(n) => n,
                    None => continue,
                };

                // Determine action
                let suggestion = self.determine_action(&node, &other, result.score)?;

                duplicates.push(DuplicatePair {
                    node_a: node.id,
                    node_b: other.id,
                    similarity: result.score,
                    suggestion,
                });
            }
        }

        Ok(DedupResult { duplicates })
    }

    /// Determine the appropriate action for a duplicate pair
    fn determine_action(&self, a: &Node, b: &Node, similarity: f32) -> Result<DedupAction> {
        // Get connection counts
        let a_connections = self.get_connection_count(a.id)?;
        let b_connections = self.get_connection_count(b.id)?;

        // If one has significantly more connections, keep it
        if a_connections > b_connections * 2 || b_connections > a_connections * 2 {
            let keep = if a_connections > b_connections {
                a.id
            } else {
                b.id
            };
            let retire = if a_connections > b_connections {
                b.id
            } else {
                a.id
            };
            return Ok(DedupAction::Merge { keep, retire });
        }

        // If one is much more important, keep it
        if (a.importance - b.importance).abs() > 0.3 {
            let keep = if a.importance > b.importance {
                a.id
            } else {
                b.id
            };
            let retire = if a.importance > b.importance {
                b.id
            } else {
                a.id
            };
            return Ok(DedupAction::Merge { keep, retire });
        }

        // If very similar (near exact duplicate), supersede by age
        if similarity >= 0.98 {
            let (newer, older) = if a.created_at > b.created_at {
                (a.id, b.id)
            } else {
                (b.id, a.id)
            };
            return Ok(DedupAction::Supersede { newer, older });
        }

        // Otherwise, just link them as related
        Ok(DedupAction::Link)
    }

    /// Get the number of edges connected to a node
    fn get_connection_count(&self, node_id: NodeId) -> Result<usize> {
        let outgoing = self.storage.edges_from(node_id)?;
        let incoming = self.storage.edges_to(node_id)?;
        Ok(outgoing.len() + incoming.len())
    }

    /// Execute a dedup action
    pub fn execute_action(&self, pair: &DuplicatePair) -> Result<()> {
        match &pair.suggestion {
            DedupAction::Merge { keep, retire } => {
                self.merge_nodes(*keep, *retire)?;
            }
            DedupAction::Supersede { newer, older } => {
                // Create supersedes edge
                let edge = Edge::new(
                    *newer,
                    *older,
                    Relation::new("supersedes").unwrap(),
                    0.95,
                    EdgeProvenance::AutoDedup {
                        similarity: pair.similarity,
                    },
                );
                self.storage.put_edge(&edge)?;
            }
            DedupAction::Link => {
                // Create related edge
                let edge = Edge::new(
                    pair.node_a,
                    pair.node_b,
                    Relation::new("related_to").unwrap(),
                    pair.similarity,
                    EdgeProvenance::AutoDedup {
                        similarity: pair.similarity,
                    },
                );
                self.storage.put_edge(&edge)?;
            }
        }
        Ok(())
    }

    /// Merge two nodes
    fn merge_nodes(&self, keep: NodeId, retire: NodeId) -> Result<()> {
        // Get both nodes
        let keep_node = self
            .storage
            .get_node(keep)?
            .ok_or_else(|| crate::error::CortexError::NodeNotFound(keep))?;
        let retire_node = self
            .storage
            .get_node(retire)?
            .ok_or_else(|| crate::error::CortexError::NodeNotFound(retire))?;

        // Transfer edges from retired node to kept node
        let outgoing = self.storage.edges_from(retire)?;
        let incoming = self.storage.edges_to(retire)?;

        for mut edge in outgoing {
            // Redirect from retired to kept
            edge.from = keep;
            // Delete edges that would become self-edges
            if edge.from == edge.to {
                self.storage.delete_edge(edge.id)?;
            } else {
                self.storage.put_edge(&edge)?;
            }
        }

        for mut edge in incoming {
            // Redirect to retired to kept
            edge.to = keep;
            // Delete edges that would become self-edges
            if edge.from == edge.to {
                self.storage.delete_edge(edge.id)?;
            } else {
                self.storage.put_edge(&edge)?;
            }
        }

        // Create supersedes edge
        let supersedes_edge = Edge::new(
            keep,
            retire,
            Relation::new("supersedes").unwrap(),
            0.95,
            EdgeProvenance::AutoDedup { similarity: 1.0 },
        );
        self.storage.put_edge(&supersedes_edge)?;

        // Merge metadata
        let mut updated_keep = keep_node.clone();

        // Union of tags
        let mut all_tags = keep_node.data.tags.clone();
        for tag in &retire_node.data.tags {
            if !all_tags.contains(tag) {
                all_tags.push(tag.clone());
            }
        }
        updated_keep.data.tags = all_tags;

        // Merge metadata maps
        for (key, value) in &retire_node.data.metadata {
            if !updated_keep.data.metadata.contains_key(key) {
                updated_keep
                    .data
                    .metadata
                    .insert(key.clone(), value.clone());
            }
        }

        // Update importance (take max)
        updated_keep.importance = keep_node.importance.max(retire_node.importance);

        self.storage.put_node(&updated_keep)?;

        // Tombstone retired node (soft delete)
        let mut tombstoned = retire_node.clone();
        tombstoned.deleted = true;
        tombstoned.updated_at = Utc::now();
        self.storage.put_node(&tombstoned)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphEngineImpl;
    use crate::storage::RedbStorage;
    use crate::types::{Node, NodeKind, Source};
    use crate::vector::{EmbeddingService, FastEmbedService, HnswIndex};
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    #[test]
    #[ignore] // Requires embedding model
    fn test_dedup_detection() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("dedup_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create near-duplicate nodes
        let node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Rust is fast".into(),
            "Rust is a fast systems programming language".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );

        let node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Rust is fast".into(),
            "Rust is a fast systems language".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Create embeddings
        let embedding_service = FastEmbedService::new().unwrap();
        let vector_index = HnswIndex::new(384);

        let mut vector_index_mut = vector_index;
        for node in [&node1, &node2] {
            let text = crate::vector::embedding_input(node);
            let emb = embedding_service.embed(&text).unwrap();
            vector_index_mut.insert(node.id, &emb).unwrap();

            // Store embedding in node
            let mut updated = node.clone();
            updated.embedding = Some(emb);
            storage.put_node(&updated).unwrap();
        }

        vector_index_mut.rebuild().unwrap();

        let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));
        let vector_index = Arc::new(RwLock::new(vector_index_mut));

        let scanner = DedupScanner::new(
            storage.clone(),
            vector_index,
            graph_engine,
            SimilarityConfig::default(),
        );

        let result = scanner.scan().unwrap();

        // Should detect the duplicate pair
        assert!(!result.duplicates.is_empty());
        assert_eq!(result.duplicates.len(), 1);
        assert!(result.duplicates[0].similarity > 0.9);
    }

    #[test]
    fn test_merge_nodes() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("merge_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        let node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 1".into(),
            "Body 1".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.8,
        );

        let mut node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 2".into(),
            "Body 2".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.6,
        );

        node2.data.tags.push("extra".into());

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Create edge from node2
        let edge = Edge::new(
            node2.id,
            node1.id,
            Relation::new("related_to").unwrap(),
            0.7,
            EdgeProvenance::Manual {
                created_by: "test".into(),
            },
        );
        storage.put_edge(&edge).unwrap();

        let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));
        let vector_index = Arc::new(RwLock::new(HnswIndex::new(384)));

        let scanner = DedupScanner::new(
            storage.clone(),
            vector_index,
            graph_engine,
            SimilarityConfig::default(),
        );

        // Merge node2 into node1
        scanner.merge_nodes(node1.id, node2.id).unwrap();

        // node2 should be tombstoned
        let node2_after = storage.get_node(node2.id).unwrap().unwrap();
        assert!(node2_after.deleted);

        // node1 should have merged metadata
        let node1_after = storage.get_node(node1.id).unwrap().unwrap();
        assert!(node1_after.data.tags.contains(&"extra".to_string()));

        // Importance should be max
        assert_eq!(node1_after.importance, 0.8);

        // Edge would have become a self-edge (node1->node1) so it should be deleted
        // Self-edges are not allowed
        let edge_after = storage.get_edge(edge.id).unwrap();
        assert!(edge_after.is_none());
    }
}
