# Cortex Architecture

This document describes the internal architecture and design decisions of Cortex.

## Overview

Cortex is a self-organizing graph memory engine built in Rust. It combines traditional graph algorithms with modern vector search to create a knowledge system that automatically discovers and maintains connections between information.

## Design Principles

1. **Embedded-First**: No external database required - redb provides ACID guarantees
2. **Zero-Copy**: Memory-mapped storage for efficient data access
3. **Self-Organizing**: Auto-linker maintains graph structure without manual curation
4. **Hybrid Intelligence**: Combines symbolic (graph) and subsymbolic (vector) reasoning
5. **Temporal Awareness**: Edge decay reflects relevance over time
6. **Production-Ready**: gRPC for performance, HTTP for debugging

## Component Architecture

### Storage Layer (`cortex-core/storage`)

**Technology**: redb (embedded ACID database)

**Design**:
- Primary table: `nodes` (NodeId → Node)
- Edge table: `edges` (EdgeId → Edge)
- Secondary indexes for filtering:
  - `nodes_by_kind` (NodeKind → Set<NodeId>)
  - `nodes_by_source` (SourceAgent → Set<NodeId>)
  - `nodes_by_session` (Session → Set<NodeId>)
  - `edges_from` (NodeId → Set<EdgeId>)
  - `edges_to` (NodeId → Set<EdgeId>)

**Trade-offs**:
- ✅ ACID transactions, MVCC, crash safety
- ✅ Zero-copy mmap for fast reads
- ✅ No external dependencies
- ⚠️ Single-machine only (not distributed)
- ⚠️ Write throughput limited by fsync

**Key Methods**:
```rust
pub trait Storage {
    fn put_node(&self, node: &Node) -> Result<()>;
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;
    fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>>;
    fn put_edge(&self, edge: &Edge) -> Result<()>;
    fn edges_from(&self, id: NodeId) -> Result<Vec<Edge>>;
    fn edges_to(&self, id: NodeId) -> Result<Vec<Edge>>;
}
```

### Graph Engine (`cortex-core/graph`)

**Purpose**: High-performance graph traversal and path finding

**Algorithms**:
1. **Breadth-First Search (BFS)**: Uniform exploration
2. **Depth-First Search (DFS)**: Deep exploration of paths
3. **Weighted Traversal**: Follows high-weight edges preferentially
4. **Dijkstra's Algorithm**: Shortest paths with edge weights
5. **Yen's k-Shortest Paths**: Alternative path discovery

**Optimization**: Adjacency cache (in-memory hash map)
- Invalidated on edge modifications
- 10x speedup for repeated traversals
- Trade memory (~10KB per node) for speed

**Use Cases**:
- Find related concepts (BFS from node)
- Trace causal chains (DFS with Causes relation)
- Discover alternative explanations (k-shortest paths)

### Vector Layer (`cortex-core/vector`)

**Embedding Model**: FastEmbed with BAAI/bge-small-en-v1.5
- 384 dimensions
- ~30ms per embedding on CPU
- English-optimized

**Index**: HNSW (Hierarchical Navigable Small World)
- M=16 (max connections per layer)
- ef_construction=200 (build quality)
- Sub-linear search time: O(log N)

**Hybrid Retrieval**:
```rust
score = α × vector_similarity + (1-α) × graph_proximity
```
- α=0.7 by default (vector-heavy)
- Prevents "similar but irrelevant" results
- Boosts nodes connected to query context

**Design Decision**: Separate vector index from storage
- Allows rebuild without data loss
- Index is ephemeral, can be reconstructed
- Storage is source of truth

### Auto-Linker (`cortex-core/linker`)

**Purpose**: Autonomous graph maintenance

**Components**:

1. **Processing Loop**:
   ```
   ┌─────────────────────────────────────┐
   │  Fetch new/updated nodes (batch)     │
   └────────────────┬────────────────────┘
                    │
   ┌────────────────▼────────────────────┐
   │  Apply similarity + structural rules │
   └────────────────┬────────────────────┘
                    │
   ┌────────────────▼────────────────────┐
   │  Detect and merge duplicates         │
   └────────────────┬────────────────────┘
                    │
   ┌────────────────▼────────────────────┐
   │  Run edge decay on old edges         │
   └─────────────────────────────────────┘
   ```

2. **Link Rules**:
   - **Similarity**: Vector similarity > 0.85 → Similar edge
   - **Temporal**: Events in sequence → Precedes edge
   - **Source**: Same session → PartOf edge
   - **Causality**: Decision before event → Causes edge
   - **Support**: Fact near decision → Supports edge
   - **Reference**: Title mentions → References edge

3. **Contradiction Detection**:
   - Finds semantically similar nodes with opposite polarity
   - Uses sentiment analysis + negation detection
   - Creates Contradicts edge for human review

4. **Deduplication**:
   - Similarity > 0.95 → near-duplicate
   - Merge strategy:
     - More connections → keep as primary
     - Transfer edges to primary
     - Mark duplicate as deleted
   - Alternative: Supersede or Link (based on importance)

5. **Edge Decay**:
   ```rust
   new_weight = original_weight * e^(-λt) * shield
   ```
   - λ = ln(2) / half_life (default: 30 days)
   - shield = max(importance, 0.5) prevents critical edge loss
   - Access reinforcement: recent use resets timer

**Concurrency**: Arc<RwLock<VectorIndex>> for thread-safe shared access

**Performance**: ~100 nodes/second (bottleneck: embeddings)

### API Layer (`cortex-server`)

**gRPC Service** (port 9090):
- Production interface
- 20+ RPC methods
- Protobuf schema in `cortex-proto`
- Methods:
  - Node CRUD: AddNode, GetNode, UpdateNode, DeleteNode
  - Edge CRUD: AddEdge, GetEdge, DeleteEdge
  - Search: Search, ListNodes, ListEdges
  - Graph: Traverse, FindPaths, GetNeighbors
  - Auto-linker: TriggerAutoLink, GetAutoLinkerStatus

**HTTP Service** (port 9091):
- Debug/exploration interface
- RESTful endpoints
- Built with Axum
- Key endpoints:
  - GET /health - Health check
  - GET /stats - Database statistics
  - GET /nodes - List nodes (filterable)
  - GET /nodes/:id - Get single node
  - GET /search?q=query - Search
  - GET /graph/viz - Interactive visualization
  - GET /graph/export - Export full graph
  - POST /auto-linker/trigger - Trigger cycle

**NATS Consumer**:
- Subscribes to `warren.>` subject
- Parses Warren events into nodes
- Auto-generates embeddings
- Deduplicates by title+session
- Event types: stage.advanced, item.completed, evidence.submitted, etc.

## Data Flow

### Ingestion Flow
```
┌─────────┐
│  gRPC   │  AddNode(title, body, ...)
│ Client  │
└────┬────┘
     │
     ▼
┌────────────────┐
│   Generate     │  FastEmbed
│   Embedding    │  (30ms)
└────┬───────────┘
     │
     ▼
┌────────────────┐
│  Store Node    │  redb transaction
│  + Embedding   │
└────┬───────────┘
     │
     ▼
┌────────────────┐
│  Index Vector  │  HNSW insert
│                │
└────────────────┘
```

### Search Flow
```
┌─────────┐
│  Query  │  "memory safety in Rust"
└────┬────┘
     │
     ▼
┌────────────────┐
│  Embed Query   │  FastEmbed
└────┬───────────┘
     │
     ▼
┌────────────────┐
│  Vector Search │  HNSW → top-K candidates
└────┬───────────┘
     │
     ▼
┌────────────────┐
│  Hybrid Score  │  Combine vector + graph
└────┬───────────┘
     │
     ▼
┌────────────────┐
│  Fetch Nodes   │  redb batch read
│  + Metadata    │
└────────────────┘
```

### Auto-Linker Cycle
```
Every N seconds (default: 300):
┌─────────────────────────────────┐
│ Fetch nodes since last cursor   │
│ (batch_size=100)                │
└────┬────────────────────────────┘
     │
     ▼
┌─────────────────────────────────┐
│ For each node:                  │
│  - Apply 6 structural rules     │
│  - Create edges if matched      │
└────┬────────────────────────────┘
     │
     ▼
┌─────────────────────────────────┐
│ Detect near-duplicates          │
│ Merge or supersede              │
└────┬────────────────────────────┘
     │
     ▼
┌─────────────────────────────────┐
│ Run decay on old edges          │
│ Delete if weight < 0.1          │
└────┬────────────────────────────┘
     │
     ▼
┌─────────────────────────────────┐
│ Update cursor, emit metrics     │
└─────────────────────────────────┘
```

## Concurrency Model

**Storage**: Read-write lock per transaction
- Multiple concurrent readers
- Single writer (serialized by redb)

**Vector Index**: Arc<RwLock<HnswIndex>>
- Read-heavy workload
- Writes only during ingestion/rebuild
- Lock contention minimal in practice

**Auto-Linker**: Background tokio task
- Runs independently every N seconds
- Acquires write lock on vector index briefly
- No coordination with API layer

**HTTP/gRPC**: Separate tokio tasks
- Share Arc'd references to storage/index
- Fully concurrent request handling

## Performance Characteristics

| Operation | Complexity | Typical Time |
|-----------|-----------|--------------|
| Put Node | O(log N) | <1ms |
| Get Node | O(1) | <0.1ms |
| Vector Search | O(log N) | <10ms (top-10) |
| BFS Traversal | O(V+E) | <5ms (depth 3) |
| Auto-link Cycle | O(N×M) | 1s per 100 nodes |
| Edge Decay | O(E) | 100ms per 10k edges |

**Scaling Limits** (single machine):
- Nodes: ~10M (storage limit: disk size)
- Edges: ~100M (memory for adjacency cache: ~10GB)
- Vector index: ~1M vectors (HNSW memory: ~1.5GB)

## Future Optimizations

1. **Distributed Storage**: Shard graph across machines
2. **GPU Embeddings**: 10x faster with CUDA
3. **Incremental HNSW**: Avoid full rebuilds
4. **Edge Compression**: Store deltas instead of full weights
5. **Query Cache**: Memoize frequent searches
6. **Async Decay**: Background thread instead of cycle

## Security Considerations

1. **Input Validation**: All external input sanitized
2. **Resource Limits**: Batch sizes, search limits
3. **No SQL Injection**: Embedded DB, no query strings
4. **Memory Safety**: Rust guarantees + no unsafe code
5. **DOS Prevention**: Rate limiting (TODO)

## Testing Strategy

1. **Unit Tests**: Per-module in `cortex-core/src/*/tests.rs`
2. **Integration Tests**: End-to-end in `cortex-server/tests/`
3. **Property Tests**: Invariants (graph consistency, embedding dimensions)
4. **Benchmarks**: Performance regression detection (TODO)

## Monitoring

**Metrics Available**:
- Storage: node count, edge count, disk usage
- Vector index: indexed count, search latency
- Auto-linker: cycles run, links created, dedup count
- API: request count, error rate (TODO)

**Logging**:
- INFO: Server start, cycle completion, major events
- DEBUG: Individual operations, rule applications
- ERROR: Failures, inconsistencies

## Deployment Patterns

**Single Instance** (current):
- Sufficient for 100k-1M nodes
- All components in one process
- Simplest deployment

**Replicated** (future):
- Read replicas for scaling queries
- Single writer for consistency
- Use redb replication (when available)

**Sharded** (future):
- Partition graph by domain/topic
- Route requests to correct shard
- Cross-shard queries via federation
