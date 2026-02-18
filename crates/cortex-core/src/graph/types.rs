use crate::types::{EdgeId, NodeId, NodeKind, Relation};
use chrono::{DateTime, Utc};

/// Request for graph traversal
#[derive(Debug, Clone)]
pub struct TraversalRequest {
    /// Starting node(s). Can start from multiple roots.
    pub start: Vec<NodeId>,

    /// Maximum depth. 0 = start nodes only. None = unlimited (dangerous, use with care).
    pub max_depth: Option<u32>,

    /// Which directions to follow edges.
    pub direction: TraversalDirection,

    /// Only follow edges with these relation types. None = all.
    pub relation_filter: Option<Vec<Relation>>,

    /// Only include nodes of these kinds in results. None = all.
    /// Note: filtering doesn't stop traversal — a filtered-out node
    /// is still traversed through, just not returned.
    pub kind_filter: Option<Vec<NodeKind>>,

    /// Minimum edge weight to follow. Edges below this are ignored.
    /// Useful for pruning weak auto-generated edges.
    pub min_weight: Option<f32>,

    /// Maximum number of nodes to return. Traversal stops early
    /// when limit is hit. None = no limit.
    pub limit: Option<usize>,

    /// Traversal algorithm.
    pub strategy: TraversalStrategy,

    /// Whether to include the start nodes in results.
    pub include_start: bool,

    /// Time boundary. Only follow edges/nodes created after this time.
    pub created_after: Option<DateTime<Utc>>,
}

impl Default for TraversalRequest {
    fn default() -> Self {
        Self {
            start: Vec::new(),
            max_depth: Some(3),
            direction: TraversalDirection::Outgoing,
            relation_filter: None,
            kind_filter: None,
            min_weight: None,
            limit: None,
            strategy: TraversalStrategy::Bfs,
            include_start: true,
            created_after: None,
        }
    }
}

/// Direction to follow edges during traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow edges where the current node is `from`.
    Outgoing,

    /// Follow edges where the current node is `to`.
    Incoming,

    /// Follow edges in both directions.
    Both,
}

/// Strategy for graph traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalStrategy {
    /// Breadth-first. Explores all neighbors at depth N before depth N+1.
    /// Best for: "what's immediately connected?"
    Bfs,

    /// Depth-first. Explores one path to its end before backtracking.
    /// Best for: "find me a chain from A to B."
    Dfs,

    /// Weighted. Prioritizes highest-weight edges first (greedy best-first).
    /// Best for: "what's most strongly connected?"
    Weighted,
}

/// Request for path finding between two nodes
#[derive(Debug, Clone)]
pub struct PathRequest {
    /// Starting node.
    pub from: NodeId,

    /// Target node.
    pub to: NodeId,

    /// Maximum path length (edges). None = unlimited.
    pub max_length: Option<u32>,

    /// Only follow these relation types.
    pub relation_filter: Option<Vec<Relation>>,

    /// Minimum edge weight on path.
    pub min_weight: Option<f32>,

    /// How many paths to return. Default 1 (shortest).
    pub max_paths: usize,
}

impl Default for PathRequest {
    fn default() -> Self {
        Self {
            from: NodeId::nil(),
            to: NodeId::nil(),
            max_length: None,
            relation_filter: None,
            min_weight: None,
            max_paths: 1,
        }
    }
}

/// Result of path finding query
#[derive(Debug, Clone)]
pub struct PathResult {
    /// Ordered list of paths, shortest first.
    pub paths: Vec<Path>,
}

/// A path through the graph
#[derive(Debug, Clone)]
pub struct Path {
    /// Ordered list of nodes along the path.
    pub nodes: Vec<NodeId>,

    /// Ordered list of edges along the path.
    pub edges: Vec<EdgeId>,

    /// Total weight (product of edge weights along path).
    pub total_weight: f32,

    /// Number of edges.
    pub length: u32,
}

impl Path {
    /// Create a new path
    pub fn new(nodes: Vec<NodeId>, edges: Vec<EdgeId>, total_weight: f32) -> Self {
        let length = edges.len() as u32;
        Self {
            nodes,
            edges,
            total_weight,
            length,
        }
    }
}

/// Configuration for traversal budgets
#[derive(Debug, Clone)]
pub struct TraversalBudget {
    /// Maximum nodes to visit before aborting
    pub max_visited: usize,

    /// Maximum time in milliseconds
    pub max_time_ms: u64,

    /// Maximum nodes at a single depth level (circuit breaker)
    pub max_nodes_per_level: usize,
}

impl Default for TraversalBudget {
    fn default() -> Self {
        Self {
            max_visited: 10_000,
            max_time_ms: 5_000,
            max_nodes_per_level: 1_000,
        }
    }
}
