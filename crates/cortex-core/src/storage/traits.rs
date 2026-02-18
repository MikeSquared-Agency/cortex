use crate::error::Result;
use crate::storage::filters::{NodeFilter, StorageStats};
use crate::types::{Edge, EdgeId, Node, NodeId};
use std::path::Path;

/// Storage trait for the graph database
pub trait Storage: Send + Sync {
    // === Node Operations ===

    /// Store a node (insert or update)
    fn put_node(&self, node: &Node) -> Result<()>;

    /// Retrieve a node by ID
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;

    /// Soft delete a node (sets tombstone flag)
    fn delete_node(&self, id: NodeId) -> Result<()>;

    /// List nodes matching the filter
    fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>>;

    /// Count nodes matching the filter
    fn count_nodes(&self, filter: NodeFilter) -> Result<u64>;

    // === Edge Operations ===

    /// Store an edge (insert or update)
    fn put_edge(&self, edge: &Edge) -> Result<()>;

    /// Retrieve an edge by ID
    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>>;

    /// Delete an edge (hard delete, edges don't use tombstones)
    fn delete_edge(&self, id: EdgeId) -> Result<()>;

    /// Get all edges originating from a node
    fn edges_from(&self, node_id: NodeId) -> Result<Vec<Edge>>;

    /// Get all edges pointing to a node
    fn edges_to(&self, node_id: NodeId) -> Result<Vec<Edge>>;

    /// Get all edges between two specific nodes
    fn edges_between(&self, from: NodeId, to: NodeId) -> Result<Vec<Edge>>;

    // === Batch Operations ===

    /// Insert or update multiple nodes in a single transaction
    fn put_nodes_batch(&self, nodes: &[Node]) -> Result<()>;

    /// Insert or update multiple edges in a single transaction
    fn put_edges_batch(&self, edges: &[Edge]) -> Result<()>;

    // === Metadata ===

    /// Store metadata key-value pair
    fn put_metadata(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Retrieve metadata by key
    fn get_metadata(&self, key: &str) -> Result<Option<Vec<u8>>>;

    // === Maintenance ===

    /// Compact the database (redb does this automatically, but exposed for control)
    fn compact(&self) -> Result<()>;

    /// Get database statistics
    fn stats(&self) -> Result<StorageStats>;

    /// Create a file-level backup/snapshot
    fn snapshot(&self, path: &Path) -> Result<()>;
}
