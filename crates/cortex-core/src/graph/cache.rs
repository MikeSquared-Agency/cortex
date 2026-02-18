use crate::error::Result;
use crate::storage::Storage;
use crate::types::{EdgeId, NodeId, Relation};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// Entry in the adjacency cache
#[derive(Debug, Clone)]
pub struct AdjacencyEntry {
    pub edge_id: EdgeId,
    pub target: NodeId,
    pub relation: Relation,
    pub weight: f32,
}

/// In-memory adjacency list cache for fast traversals
pub struct AdjacencyCache {
    /// Outgoing edges: node_id → Vec<(edge_id, target_node_id, relation, weight)>
    outgoing: RwLock<HashMap<NodeId, Vec<AdjacencyEntry>>>,

    /// Incoming edges: node_id → Vec<(edge_id, source_node_id, relation, weight)>
    incoming: RwLock<HashMap<NodeId, Vec<AdjacencyEntry>>>,

    /// Invalidated on any write. Rebuilt lazily on next read.
    valid: AtomicBool,
}

impl AdjacencyCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            outgoing: RwLock::new(HashMap::new()),
            incoming: RwLock::new(HashMap::new()),
            valid: AtomicBool::new(false),
        }
    }

    /// Check if the cache is valid
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }

    /// Invalidate the cache (call after any write operation)
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }

    /// Build the cache from storage
    pub fn build<S: Storage>(&self, storage: &S) -> Result<()> {
        // Get all nodes
        let nodes = storage.list_nodes(crate::storage::NodeFilter::new())?;

        let mut outgoing_map = HashMap::new();
        let mut incoming_map = HashMap::new();

        // Build adjacency lists
        for node in nodes {
            if node.deleted {
                continue;
            }

            // Get outgoing edges
            let outgoing_edges = storage.edges_from(node.id)?;
            let outgoing_entries: Vec<AdjacencyEntry> = outgoing_edges
                .into_iter()
                .map(|e| AdjacencyEntry {
                    edge_id: e.id,
                    target: e.to,
                    relation: e.relation,
                    weight: e.weight,
                })
                .collect();

            outgoing_map.insert(node.id, outgoing_entries);

            // Get incoming edges
            let incoming_edges = storage.edges_to(node.id)?;
            let incoming_entries: Vec<AdjacencyEntry> = incoming_edges
                .into_iter()
                .map(|e| AdjacencyEntry {
                    edge_id: e.id,
                    target: e.from,
                    relation: e.relation,
                    weight: e.weight,
                })
                .collect();

            incoming_map.insert(node.id, incoming_entries);
        }

        // Update the cache
        *self.outgoing.write().unwrap() = outgoing_map;
        *self.incoming.write().unwrap() = incoming_map;

        // Mark as valid
        self.valid.store(true, Ordering::Release);

        Ok(())
    }

    /// Get outgoing edges for a node (from cache)
    pub fn get_outgoing(&self, node_id: NodeId) -> Option<Vec<AdjacencyEntry>> {
        if !self.is_valid() {
            return None;
        }

        let cache = self.outgoing.read().unwrap();
        cache.get(&node_id).cloned()
    }

    /// Get incoming edges for a node (from cache)
    pub fn get_incoming(&self, node_id: NodeId) -> Option<Vec<AdjacencyEntry>> {
        if !self.is_valid() {
            return None;
        }

        let cache = self.incoming.read().unwrap();
        cache.get(&node_id).cloned()
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let outgoing = self.outgoing.read().unwrap();
        let incoming = self.incoming.read().unwrap();

        let node_count = outgoing.len().max(incoming.len());
        let edge_count = outgoing.values().map(|v| v.len()).sum::<usize>();

        CacheStats {
            valid: self.is_valid(),
            node_count,
            edge_count,
        }
    }

    /// Clear the cache
    pub fn clear(&self) {
        self.outgoing.write().unwrap().clear();
        self.incoming.write().unwrap().clear();
        self.invalidate();
    }
}

impl Default for AdjacencyCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub valid: bool,
    pub node_count: usize,
    pub edge_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, Node, NodeKind, Relation, Source};
    use tempfile::TempDir;

    fn create_test_storage() -> (RedbStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("cache_test.redb");
        let storage = RedbStorage::open(&db_path).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_cache_build() {
        let (storage, _temp) = create_test_storage();

        // Create test nodes
        let node1 = Node::new(
            NodeKind::Fact,
            "Node 1".to_string(),
            "Test".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );
        let node2 = Node::new(
            NodeKind::Fact,
            "Node 2".to_string(),
            "Test".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Create edge
        let edge = Edge::new(
            node1.id,
            node2.id,
            Relation::RelatedTo,
            0.8,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        // Build cache
        let cache = AdjacencyCache::new();
        assert!(!cache.is_valid());

        cache.build(&storage).unwrap();
        assert!(cache.is_valid());

        // Check outgoing edges
        let outgoing = cache.get_outgoing(node1.id).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].target, node2.id);
        assert_eq!(outgoing[0].relation, Relation::RelatedTo);

        // Check incoming edges
        let incoming = cache.get_incoming(node2.id).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].target, node1.id);
    }

    #[test]
    fn test_cache_invalidation() {
        let (storage, _temp) = create_test_storage();

        let cache = AdjacencyCache::new();
        cache.build(&storage).unwrap();
        assert!(cache.is_valid());

        cache.invalidate();
        assert!(!cache.is_valid());

        // Cache should return None when invalid
        let result = cache.get_outgoing(uuid::Uuid::now_v7());
        assert!(result.is_none());
    }
}
