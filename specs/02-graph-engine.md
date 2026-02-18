# Phase 2 — Graph Engine: Traversal & Query

**Duration:** 1 week  
**Crate:** `cortex-core` (extends Phase 1)  
**Dependencies:** Phase 1 complete  
**New deps:** None (pure algorithmic code on top of storage)

---

## Objective

Build the graph traversal and query engine. This is what makes it a *graph* database and not just a key-value store with indexes. The engine answers questions like "what chain of decisions led to this outcome?" and "what does agent X need to know?"

---

## Core Concepts

### Traversal vs Query

- **Traversal** = start at a node, walk edges, collect results. "Starting from this decision, follow all `led_to` edges 3 hops deep."
- **Query** = filter the graph globally, then optionally traverse from results. "Find all patterns tagged 'infrastructure', then get everything within 2 hops."

Both return subgraphs — a set of nodes and the edges connecting them.

---

## Traversal Engine

### TraversalRequest

```rust
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
```

### TraversalDirection

```rust
pub enum TraversalDirection {
    /// Follow edges where the current node is `from`.
    Outgoing,
    
    /// Follow edges where the current node is `to`.
    Incoming,
    
    /// Follow edges in both directions.
    Both,
}
```

### TraversalStrategy

```rust
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
```

### Subgraph (Result)

```rust
pub struct Subgraph {
    /// All nodes in the result, keyed by ID for O(1) lookup.
    pub nodes: HashMap<NodeId, Node>,
    
    /// All edges connecting the result nodes.
    pub edges: Vec<Edge>,
    
    /// Depth of each node from the nearest start node.
    pub depths: HashMap<NodeId, u32>,
    
    /// Total nodes visited during traversal (may be > nodes.len()
    /// if kind_filter excluded some).
    pub visited_count: usize,
    
    /// Whether traversal was truncated by limit.
    pub truncated: bool,
}

impl Subgraph {
    /// Get all nodes at a specific depth.
    pub fn at_depth(&self, depth: u32) -> Vec<&Node>;
    
    /// Get all edges between two specific nodes.
    pub fn edges_between(&self, a: NodeId, b: NodeId) -> Vec<&Edge>;
    
    /// Get neighbors of a node within this subgraph.
    pub fn neighbors(&self, id: NodeId) -> Vec<&Node>;
    
    /// Topological sort (if DAG). Returns None if cycles exist.
    pub fn topo_sort(&self) -> Option<Vec<NodeId>>;
    
    /// Merge another subgraph into this one.
    pub fn merge(&mut self, other: Subgraph);
}
```

---

## Path Queries

Find paths between two specific nodes.

```rust
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

pub struct PathResult {
    /// Ordered list of paths, shortest first.
    pub paths: Vec<Path>,
}

pub struct Path {
    /// Alternating node-edge-node sequence.
    pub nodes: Vec<NodeId>,
    pub edges: Vec<EdgeId>,
    
    /// Total weight (product of edge weights along path).
    pub total_weight: f32,
    
    /// Number of edges.
    pub length: u32,
}
```

Implementation: BFS for unweighted shortest path, Dijkstra (inverted weights — higher weight = lower cost) for weighted shortest path. Yen's algorithm for k-shortest paths when `max_paths > 1`.

---

## Neighborhood Queries

Convenience layer on top of traversal for common patterns.

```rust
pub trait GraphEngine: Send + Sync {
    /// Core traversal. Everything else builds on this.
    fn traverse(&self, request: TraversalRequest) -> Result<Subgraph>;
    
    /// Find paths between two nodes.
    fn find_paths(&self, request: PathRequest) -> Result<PathResult>;
    
    // --- Convenience methods ---
    
    /// Direct neighbors of a node (depth 1).
    fn neighbors(
        &self, 
        id: NodeId, 
        direction: TraversalDirection,
        relation_filter: Option<Vec<Relation>>,
    ) -> Result<Vec<(Node, Edge)>>;
    
    /// Everything within N hops of a node.
    fn neighborhood(&self, id: NodeId, depth: u32) -> Result<Subgraph>;
    
    /// All nodes that a given node can reach (transitive closure).
    fn reachable(&self, id: NodeId, direction: TraversalDirection) -> Result<Vec<NodeId>>;
    
    /// Find all nodes with no incoming edges of a given relation.
    /// Useful for finding "root causes" or "original decisions."
    fn roots(&self, relation: Relation) -> Result<Vec<Node>>;
    
    /// Find all nodes with no outgoing edges of a given relation.
    /// Useful for finding "leaf outcomes" or "terminal states."
    fn leaves(&self, relation: Relation) -> Result<Vec<Node>>;
    
    /// Detect cycles in the graph (or within a subgraph).
    fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>>;
    
    /// Connected components. Groups of nodes that can reach each other.
    fn components(&self) -> Result<Vec<Vec<NodeId>>>;
    
    /// Degree centrality: which nodes have the most connections?
    /// Returns nodes sorted by total edge count (in + out).
    fn most_connected(&self, limit: usize) -> Result<Vec<(Node, usize)>>;
}
```

---

## Temporal Queries

The graph changes over time. These queries answer "what happened when?"

```rust
pub trait TemporalQueries: Send + Sync {
    /// Nodes created or updated since a given timestamp.
    /// Used by auto-linker to find new nodes to process.
    fn changed_since(&self, since: DateTime<Utc>) -> Result<Vec<Node>>;
    
    /// Snapshot of a node's neighborhood at a point in time.
    /// Only includes nodes and edges that existed at `at`.
    fn neighborhood_at(
        &self, 
        id: NodeId, 
        depth: u32, 
        at: DateTime<Utc>,
    ) -> Result<Subgraph>;
    
    /// Timeline: ordered list of nodes created within a time range.
    fn timeline(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        kind_filter: Option<Vec<NodeKind>>,
    ) -> Result<Vec<Node>>;
}
```

---

## Query Composition

Complex queries compose from primitives. No custom query language needed — Rust's type system IS the query language.

### Example: "What decisions led to patterns that apply to Kai?"

```rust
// Step 1: Find the Kai agent node
let kai = storage.list_nodes(NodeFilter {
    kinds: Some(vec![NodeKind::Agent]),
    tags: Some(vec!["kai".into()]),
    ..Default::default()
})?;

// Step 2: Traverse incoming AppliesTo edges to find patterns
let patterns = engine.traverse(TraversalRequest {
    start: vec![kai[0].id],
    max_depth: Some(1),
    direction: TraversalDirection::Incoming,
    relation_filter: Some(vec![Relation::AppliesTo]),
    kind_filter: Some(vec![NodeKind::Pattern]),
    ..Default::default()
})?;

// Step 3: From those patterns, follow incoming LedTo edges to find decisions
let decisions = engine.traverse(TraversalRequest {
    start: patterns.nodes.keys().cloned().collect(),
    max_depth: Some(1),
    direction: TraversalDirection::Incoming,
    relation_filter: Some(vec![Relation::LedTo]),
    kind_filter: Some(vec![NodeKind::Decision]),
    ..Default::default()
})?;

// Or, do it in one traversal:
let full_chain = engine.traverse(TraversalRequest {
    start: vec![kai[0].id],
    max_depth: Some(3),
    direction: TraversalDirection::Incoming,
    relation_filter: Some(vec![Relation::AppliesTo, Relation::LedTo]),
    ..Default::default()
})?;
```

---

## Performance Considerations

### Adjacency Cache

For hot traversals, maintain an in-memory adjacency list cache. redb's mmap means reads are already fast, but deserializing edges repeatedly during deep traversals is wasteful.

```rust
pub struct AdjacencyCache {
    /// node_id → Vec<(edge_id, target_node_id, relation, weight)>
    outgoing: HashMap<NodeId, Vec<AdjacencyEntry>>,
    incoming: HashMap<NodeId, Vec<AdjacencyEntry>>,
    
    /// Invalidated on any write. Rebuilt lazily on next read.
    valid: AtomicBool,
}
```

This is a read-optimized cache. Invalidated on any write transaction. Rebuilt on demand. For a graph with 100k nodes and 500k edges, the cache fits comfortably in ~200MB RAM and makes traversals ~10x faster than going through redb on every hop.

### Traversal Budget

Every traversal has a hard budget to prevent runaway queries:

- **Max visited nodes:** 10,000 (configurable)
- **Max time:** 5 seconds (configurable)
- **Circuit breaker:** if a traversal visits >1000 nodes at a single depth level, something is wrong — abort and return partial results with a warning.

---

## Testing Strategy

### Unit Tests

- BFS traversal produces correct depth assignments
- DFS traversal visits nodes in correct order
- Weighted traversal prioritizes high-weight edges
- Direction filtering (outgoing only, incoming only, both)
- Relation filtering (only follow specific edge types)
- Kind filtering (only return specific node types but traverse through all)
- Weight threshold (skip weak edges)
- Depth limiting (stop at max_depth)
- Result limiting (stop at N nodes)
- Path finding: shortest path in a simple graph
- Path finding: no path exists → empty result
- Path finding: k-shortest paths (Yen's algorithm)
- Cycle detection in graphs with and without cycles
- Connected components on disconnected graph
- Temporal filtering (nodes/edges created after timestamp)
- Empty graph → empty results (no panics)

### Integration Tests

- Build a realistic graph (50 nodes, 200 edges mimicking Warren's knowledge) and run all query types
- Concurrent reads during write: 10 reader threads traversing while writer adds nodes
- Adjacency cache invalidation: verify cache rebuilds after writes

### Benchmarks

- 3-hop BFS traversal from a node with fanout 10 at each level (1000 nodes visited) — target: <10ms
- Shortest path in 10k-node random graph — target: <50ms
- Full connected components analysis on 10k nodes — target: <100ms
- Adjacency cache rebuild for 100k edges — target: <500ms

---

## Deliverables

1. `GraphEngine` trait implementation in `cortex-core`
2. BFS, DFS, and weighted traversal strategies
3. Path finding (shortest, k-shortest)
4. Temporal query support
5. Adjacency cache with automatic invalidation
6. Traversal budget enforcement
7. All convenience methods (neighbors, roots, leaves, most_connected, components)
8. Benchmark baselines established
