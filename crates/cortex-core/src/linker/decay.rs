use crate::error::Result;
use crate::linker::DecayConfig;
use crate::storage::Storage;
use crate::types::{Edge, EdgeId, EdgeProvenance, NodeId};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Engine for applying decay to edges over time
pub struct DecayEngine<S: Storage> {
    storage: Arc<S>,
    config: DecayConfig,
}

impl<S: Storage> DecayEngine<S> {
    pub fn new(storage: Arc<S>, config: DecayConfig) -> Self {
        Self { storage, config }
    }

    /// Apply decay to all edges in the graph
    /// Returns (pruned_count, deleted_count)
    pub fn apply_decay(&self, now: DateTime<Utc>) -> Result<(u64, u64)> {
        let mut pruned_count = 0;
        let mut deleted_count = 0;

        // Get all edges - we need to iterate through nodes and collect their edges
        let all_nodes = self.storage.list_nodes(crate::storage::NodeFilter::new())?;
        let mut all_edges = Vec::new();
        for node in all_nodes {
            let outgoing = self.storage.edges_from(node.id)?;
            for edge in outgoing {
                all_edges.push(edge);
            }
        }

        for edge in all_edges {
            // Skip manual edges if configured
            if self.config.exempt_manual && matches!(edge.provenance, EdgeProvenance::Manual { .. })
            {
                continue;
            }

            let mut updated_edge = edge.clone();
            let should_delete = self.apply_decay_to_edge(&mut updated_edge, now)?;

            if should_delete {
                // Delete edge
                self.storage.delete_edge(updated_edge.id)?;
                deleted_count += 1;
            } else if updated_edge.weight != edge.weight {
                // Weight changed, update edge
                self.storage.put_edge(&updated_edge)?;

                if updated_edge.weight < self.config.prune_threshold {
                    pruned_count += 1;
                }
            }
        }

        Ok((pruned_count, deleted_count))
    }

    /// Apply decay to a single edge
    /// Returns true if edge should be deleted
    fn apply_decay_to_edge(&self, edge: &mut Edge, now: DateTime<Utc>) -> Result<bool> {
        let days_since_update = (now - edge.updated_at).num_seconds() as f32 / 86400.0;

        if days_since_update <= 0.0 {
            return Ok(false);
        }

        // Get importance of connected nodes for shielding
        let from_node = self.storage.get_node(edge.from)?;
        let to_node = self.storage.get_node(edge.to)?;

        let from_importance = from_node.map(|n| n.importance).unwrap_or(0.0);
        let to_importance = to_node.map(|n| n.importance).unwrap_or(0.0);
        let max_importance = from_importance.max(to_importance);

        // Calculate effective decay rate with importance shielding
        let effective_rate =
            self.config.daily_decay_rate * (1.0 - max_importance * self.config.importance_shield);

        // Apply exponential decay
        let decay_factor = (-effective_rate * days_since_update).exp();
        edge.weight *= decay_factor;

        // Check if below delete threshold
        if edge.weight < self.config.delete_threshold {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Reinforce edges connected to a node (resets decay timer)
    pub fn reinforce(&self, node_id: NodeId) -> Result<u64> {
        let now = Utc::now();

        // Collect all edges and update timestamps
        let outgoing = self.storage.edges_from(node_id)?;
        let incoming = self.storage.edges_to(node_id)?;

        let updated_edges: Vec<Edge> = outgoing.into_iter()
            .chain(incoming.into_iter())
            .map(|mut edge| {
                edge.updated_at = now;
                edge
            })
            .collect();

        let reinforced_count = updated_edges.len() as u64;

        // Batch update all edges
        if !updated_edges.is_empty() {
            self.storage.put_edges_batch(&updated_edges)?;
        }

        // Update node access count
        if let Some(mut node) = self.storage.get_node(node_id)? {
            node.access_count += 1;
            node.updated_at = now;
            self.storage.put_node(&node)?;
        }

        Ok(reinforced_count)
    }

    /// Get edges that are pruned (below prune threshold)
    pub fn get_pruned_edges(&self) -> Result<Vec<EdgeId>> {
        let mut pruned = Vec::new();
        // Get all edges - we need to iterate through nodes and collect their edges
        let all_nodes = self.storage.list_nodes(crate::storage::NodeFilter::new())?;
        let mut all_edges = Vec::new();
        for node in all_nodes {
            let outgoing = self.storage.edges_from(node.id)?;
            for edge in outgoing {
                all_edges.push(edge);
            }
        }

        for edge in all_edges {
            if edge.weight < self.config.prune_threshold {
                pruned.push(edge.id);
            }
        }

        Ok(pruned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, Node, NodeKind, Relation, Source};
    use chrono::Duration;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_decay_reduces_weight() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("decay_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create nodes and edge
        let node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 1".into(),
            "Body 1".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        let node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 2".into(),
            "Body 2".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        let mut edge = Edge::new(
            node1.id,
            node2.id,
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::AutoSimilarity { score: 0.8 },
        );

        // Set edge to be 10 days old
        edge.updated_at = Utc::now() - Duration::days(10);
        storage.put_edge(&edge).unwrap();

        let config = DecayConfig::default();
        let decay_engine = DecayEngine::new(storage.clone(), config);

        // Apply decay
        let (_pruned, deleted) = decay_engine.apply_decay(Utc::now()).unwrap();

        // Check that weight was reduced
        let updated_edge = storage.get_edge(edge.id).unwrap().unwrap();
        assert!(updated_edge.weight < 0.8);
        assert!(updated_edge.weight > 0.0);

        // Should not be deleted (above threshold)
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_manual_edge_exempt() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("manual_exempt_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create nodes and manual edge
        let node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 1".into(),
            "Body 1".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        let node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 2".into(),
            "Body 2".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        let mut edge = Edge::new(
            node1.id,
            node2.id,
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "user".into(),
            },
        );

        // Set edge to be 100 days old
        edge.updated_at = Utc::now() - Duration::days(100);
        storage.put_edge(&edge).unwrap();

        let config = DecayConfig::default();
        let decay_engine = DecayEngine::new(storage.clone(), config);

        // Apply decay
        decay_engine.apply_decay(Utc::now()).unwrap();

        // Manual edge should not decay
        let updated_edge = storage.get_edge(edge.id).unwrap().unwrap();
        assert_eq!(updated_edge.weight, 0.8);
    }

    #[test]
    fn test_reinforce_resets_timer() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("reinforce_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create nodes and edge
        let node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 1".into(),
            "Body 1".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        let node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Node 2".into(),
            "Body 2".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        let mut edge = Edge::new(
            node1.id,
            node2.id,
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::AutoSimilarity { score: 0.8 },
        );

        let old_time = Utc::now() - Duration::days(10);
        edge.updated_at = old_time;
        storage.put_edge(&edge).unwrap();

        let config = DecayConfig::default();
        let decay_engine = DecayEngine::new(storage.clone(), config);

        // Reinforce
        let count = decay_engine.reinforce(node1.id).unwrap();
        assert_eq!(count, 1);

        // Check that updated_at was reset
        let reinforced_edge = storage.get_edge(edge.id).unwrap().unwrap();
        assert!(reinforced_edge.updated_at > old_time);
    }
}

#[cfg(test)]
mod importance_tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, Node, NodeKind, Relation, Source};
    use chrono::Duration;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_importance_shielding_actually_works() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("shield_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // High importance node
        let high = Node::new(
            NodeKind::new("decision").unwrap(), "Important".into(), "Critical decision".into(),
            Source { agent: "test".into(), session: None, channel: None }, 0.95,
        );
        // Low importance node
        let low = Node::new(
            NodeKind::new("observation").unwrap(), "Trivial".into(), "Minor observation".into(),
            Source { agent: "test".into(), session: None, channel: None }, 0.1,
        );
        storage.put_node(&high).unwrap();
        storage.put_node(&low).unwrap();

        // Two edges with same initial weight, same age
        let mut edge_important = Edge::new(
            high.id, low.id, Relation::new("led_to").unwrap(), 0.8,
            EdgeProvenance::AutoSimilarity { score: 0.8 },
        );
        edge_important.updated_at = Utc::now() - Duration::days(30);

        let other_low = Node::new(
            NodeKind::new("observation").unwrap(), "Other low".into(), "Another minor one".into(),
            Source { agent: "test".into(), session: None, channel: None }, 0.1,
        );
        storage.put_node(&other_low).unwrap();

        let mut edge_unimportant = Edge::new(
            low.id, other_low.id, Relation::new("related_to").unwrap(), 0.8,
            EdgeProvenance::AutoSimilarity { score: 0.8 },
        );
        edge_unimportant.updated_at = Utc::now() - Duration::days(30);

        storage.put_edge(&edge_important).unwrap();
        storage.put_edge(&edge_unimportant).unwrap();

        let config = DecayConfig::default();
        let engine = DecayEngine::new(storage.clone(), config);
        engine.apply_decay(Utc::now()).unwrap();

        let important_after = storage.get_edge(edge_important.id).unwrap().unwrap();
        let unimportant_after = storage.get_edge(edge_unimportant.id).unwrap().unwrap();

        // Important edge should have decayed LESS
        assert!(important_after.weight > unimportant_after.weight,
            "Important edge ({}) should decay slower than unimportant edge ({})",
            important_after.weight, unimportant_after.weight);
    }

    #[test]
    fn test_very_old_low_importance_edge_deleted() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("delete_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        let n1 = Node::new(
            NodeKind::new("observation").unwrap(), "Old".into(), "body".into(),
            Source { agent: "test".into(), session: None, channel: None }, 0.1,
        );
        let n2 = Node::new(
            NodeKind::new("observation").unwrap(), "Also old".into(), "body".into(),
            Source { agent: "test".into(), session: None, channel: None }, 0.1,
        );
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();

        let mut edge = Edge::new(
            n1.id, n2.id, Relation::new("related_to").unwrap(), 0.1, // Already weak
            EdgeProvenance::AutoSimilarity { score: 0.1 },
        );
        edge.updated_at = Utc::now() - Duration::days(365); // A year old
        storage.put_edge(&edge).unwrap();

        let config = DecayConfig::default();
        let engine = DecayEngine::new(storage.clone(), config);
        let (_, deleted) = engine.apply_decay(Utc::now()).unwrap();

        assert_eq!(deleted, 1);
        assert!(storage.get_edge(edge.id).unwrap().is_none());
    }
}
