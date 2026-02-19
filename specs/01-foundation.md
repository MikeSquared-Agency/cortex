> **Status:** IMPLEMENTED

# Phase 1 — Foundation: Storage & Data Model

**Duration:** 1 week  
**Crate:** `cortex-core`  
**Dependencies:** redb, serde, bincode, uuid, chrono

---

## Objective

Build the embedded storage layer and core data model. This is the foundation everything else sits on. Get the data structures right here and the rest follows. Get them wrong and we're refactoring for months.

---

## Data Model

### Node

```rust
pub struct Node {
    /// Unique identifier. UUIDv7 for time-sortability.
    pub id: NodeId,
    
    /// What kind of knowledge this represents.
    pub kind: NodeKind,
    
    /// The actual content. Structured but flexible.
    pub data: NodeData,
    
    /// Optional pre-computed embedding vector.
    /// None until the vector layer processes this node.
    pub embedding: Option<Embedding>,
    
    /// Which agent or process created this node.
    pub source: Source,
    
    /// Importance score (0.0 - 1.0). Affects retrieval ranking
    /// and decay rate. Higher importance decays slower.
    pub importance: f32,
    
    /// How many times this node has been accessed/referenced.
    /// Used for reinforcement — frequently accessed nodes
    /// resist decay.
    pub access_count: u64,
    
    /// When this knowledge was created.
    pub created_at: DateTime<Utc>,
    
    /// Last time this node was modified or accessed.
    pub updated_at: DateTime<Utc>,
    
    /// Soft delete. Nodes are never physically removed,
    /// only tombstoned. Allows undo and audit.
    pub deleted: bool,
}
```

### NodeKind

Eight typed memory categories. Every node must be exactly one kind. This is a closed enum — adding a new kind is a deliberate architectural decision, not a casual change.

```rust
pub enum NodeKind {
    /// Who an agent is. Identity, personality, capabilities.
    /// Example: "Kai is the orchestrator. Opus 4.6. King of Warren."
    Agent,
    
    /// A choice that was made and why.
    /// Example: "Chose Go over Rust for Dispatch because I/O-bound workload."
    Decision,
    
    /// A verified piece of information.
    /// Example: "Dispatch runs on port 8600."
    Fact,
    
    /// Something that happened at a specific time.
    /// Example: "PromptForge went down at 03:00 due to DNS failure."
    Event,
    
    /// A desired outcome or target.
    /// Example: "£3k/month from assets by end of 2026."
    Goal,
    
    /// How someone or something prefers to operate.
    /// Example: "Mike prefers casual communication, no BS."
    Preference,
    
    /// A recurring observation distilled into a rule.
    /// Example: "Workers without explicit integration instructions miss wiring."
    Pattern,
    
    /// A one-time observation not yet elevated to pattern.
    /// Example: "Correction rate dropped from 47.8% to 31.6% this week."
    Observation,
}
```

### NodeData

```rust
pub struct NodeData {
    /// Human-readable title/summary. Required.
    /// Max 256 chars. Used for display and quick scanning.
    pub title: String,
    
    /// Full content. Required.
    /// No max length but embedding quality degrades past ~2000 chars.
    /// For large content, store a summary here and link to
    /// the full source via metadata.
    pub body: String,
    
    /// Arbitrary key-value metadata. Optional.
    /// Use for: source URLs, file paths, commit SHAs, 
    /// agent IDs, task IDs, timestamps of the thing described.
    pub metadata: HashMap<String, Value>,
    
    /// Tags for lightweight categorisation.
    /// Not a replacement for NodeKind — tags are ad-hoc,
    /// kinds are structural.
    pub tags: Vec<String>,
}
```

### Edge

```rust
pub struct Edge {
    /// Unique identifier. UUIDv7.
    pub id: EdgeId,
    
    /// Source node.
    pub from: NodeId,
    
    /// Target node.
    pub to: NodeId,
    
    /// What this relationship means.
    pub relation: Relation,
    
    /// Strength of the relationship (0.0 - 1.0).
    /// Auto-created edges start at the similarity score.
    /// Manual edges start at 1.0.
    /// Decays over time unless reinforced by access.
    pub weight: f32,
    
    /// How this edge was created.
    pub provenance: EdgeProvenance,
    
    /// When this edge was created.
    pub created_at: DateTime<Utc>,
    
    /// Last time weight was updated (access or decay).
    pub updated_at: DateTime<Utc>,
}
```

### Relation

```rust
pub enum Relation {
    /// A informed B. Directional. Knowledge flow.
    /// "This decision was informed by this fact."
    InformedBy,
    
    /// A caused or resulted in B. Directional. Causality.
    /// "This event led to this decision."
    LedTo,
    
    /// A is relevant to B. Bidirectional in practice.
    /// "This pattern applies to this agent."
    AppliesTo,
    
    /// A and B contain conflicting information.
    /// "This fact contradicts this other fact."
    Contradicts,
    
    /// A replaces B. B is outdated. Directional.
    /// "This new decision supersedes the old one."
    Supersedes,
    
    /// A requires B to be true/present. Directional.
    /// "This goal depends on this fact being true."
    DependsOn,
    
    /// A and B are about the same topic. Bidirectional.
    /// Typically auto-created by similarity threshold.
    RelatedTo,
    
    /// A is an instance/example of B. Directional.
    /// "This event is an instance of this pattern."
    InstanceOf,
}
```

### EdgeProvenance

```rust
pub enum EdgeProvenance {
    /// Created explicitly by an agent or human.
    Manual { created_by: String },
    
    /// Created automatically by the auto-linker
    /// based on embedding similarity.
    AutoSimilarity { score: f32 },
    
    /// Created automatically by the auto-linker
    /// based on structural rules (e.g., same tags,
    /// same source, temporal proximity).
    AutoStructural { rule: String },
    
    /// Imported from an external source (Alexandria migration).
    Imported { source: String },
}
```

### Source

```rust
pub struct Source {
    /// Which agent created this. "kai", "dutybound", "worker-123", "human".
    pub agent: String,
    
    /// Which session/conversation. Optional.
    pub session: Option<String>,
    
    /// Which channel. Optional. "whatsapp", "slack", "terminal".
    pub channel: Option<String>,
}
```

### Type Aliases

```rust
pub type NodeId = uuid::Uuid;
pub type EdgeId = uuid::Uuid;
pub type Embedding = Vec<f32>;  // Dimension depends on model, typically 384
```

---

## Storage Layer

### Engine: redb

redb is a pure-Rust embedded key-value store with ACID transactions, MVCC (multiple concurrent readers, single writer), and zero-copy mmap reads. No WAL — writes go directly to the database file with copy-on-write semantics.

### Table Layout

```rust
// Primary tables
const NODES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("nodes");
const EDGES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("edges");

// Index tables — secondary indexes for query performance
const NODES_BY_KIND: MultimapTableDefinition<u8, &[u8; 16]> = 
    MultimapTableDefinition::new("nodes_by_kind");
const EDGES_BY_FROM: MultimapTableDefinition<&[u8; 16], &[u8; 16]> = 
    MultimapTableDefinition::new("edges_by_from");
const EDGES_BY_TO: MultimapTableDefinition<&[u8; 16], &[u8; 16]> = 
    MultimapTableDefinition::new("edges_by_to");
const NODES_BY_TAG: MultimapTableDefinition<&str, &[u8; 16]> = 
    MultimapTableDefinition::new("nodes_by_tag");
const NODES_BY_SOURCE: MultimapTableDefinition<&str, &[u8; 16]> = 
    MultimapTableDefinition::new("nodes_by_source");

// Metadata table — stores config, stats, migration version
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
```

### Serialization

Nodes and edges are serialized to bytes using **bincode** via serde. Bincode is compact (no field names, no delimiters), fast (zero-allocation deserialization with borrowing), and deterministic.

For the `metadata: HashMap<String, Value>` field, `Value` is `serde_json::Value`. This gets serialized as part of the bincode envelope — JSON flexibility inside binary efficiency.

### Storage Trait

```rust
pub trait Storage: Send + Sync {
    // Nodes
    fn put_node(&self, node: &Node) -> Result<()>;
    fn get_node(&self, id: NodeId) -> Result<Option<Node>>;
    fn delete_node(&self, id: NodeId) -> Result<()>;  // Soft delete (tombstone)
    fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>>;
    fn count_nodes(&self, filter: NodeFilter) -> Result<u64>;
    
    // Edges
    fn put_edge(&self, edge: &Edge) -> Result<()>;
    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>>;
    fn delete_edge(&self, id: EdgeId) -> Result<()>;
    fn edges_from(&self, node_id: NodeId) -> Result<Vec<Edge>>;
    fn edges_to(&self, node_id: NodeId) -> Result<Vec<Edge>>;
    fn edges_between(&self, from: NodeId, to: NodeId) -> Result<Vec<Edge>>;
    
    // Batch operations
    fn put_nodes_batch(&self, nodes: &[Node]) -> Result<()>;
    fn put_edges_batch(&self, edges: &[Edge]) -> Result<()>;
    
    // Maintenance
    fn compact(&self) -> Result<()>;
    fn stats(&self) -> Result<StorageStats>;
    fn snapshot(&self, path: &Path) -> Result<()>;  // File-level backup
}
```

### NodeFilter

```rust
pub struct NodeFilter {
    pub kinds: Option<Vec<NodeKind>>,
    pub tags: Option<Vec<String>>,
    pub source_agent: Option<String>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub min_importance: Option<f32>,
    pub include_deleted: bool,  // Default false
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
```

### StorageStats

```rust
pub struct StorageStats {
    pub node_count: u64,
    pub edge_count: u64,
    pub node_counts_by_kind: HashMap<NodeKind, u64>,
    pub edge_counts_by_relation: HashMap<Relation, u64>,
    pub db_size_bytes: u64,
    pub oldest_node: Option<DateTime<Utc>>,
    pub newest_node: Option<DateTime<Utc>>,
}
```

---

## Implementation Details

### File Location

Default: `$CORTEX_DATA_DIR/cortex.redb` or `./data/cortex.redb`

Configurable via environment variable or CLI flag. Single file. Portable — copy the file, copy the entire graph.

### Concurrency Model

redb supports multiple concurrent readers with a single writer. This maps perfectly to our access pattern:

- **Reads** (graph traversal, briefing synthesis, similarity search) — concurrent, lock-free
- **Writes** (node creation, edge updates, auto-linker) — serialized through a single write transaction

For the auto-linker (Phase 4), we'll batch writes: scan with a read transaction, compute new edges, then apply all changes in a single write transaction. This avoids write contention.

### ID Generation

UUIDv7 for both nodes and edges. Time-sortable, globally unique, no coordination needed. The time component means we can do temporal range scans on the primary key without a secondary index.

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum CortexError {
    #[error("Storage error: {0}")]
    Storage(#[from] redb::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    
    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),
    
    #[error("Edge not found: {0}")]
    EdgeNotFound(EdgeId),
    
    #[error("Invalid edge: {reason}")]
    InvalidEdge { reason: String },
    
    #[error("Duplicate node: {0}")]
    DuplicateNode(NodeId),
}
```

### Validation Rules

1. **Edge integrity** — both `from` and `to` nodes must exist (not tombstoned) when creating an edge.
2. **Self-edges prohibited** — `from` and `to` cannot be the same node.
3. **Duplicate edge detection** — no two edges with the same `(from, to, relation)` tuple. If an auto-linker tries to create a duplicate, update the weight instead.
4. **Importance range** — clamped to `[0.0, 1.0]`.
5. **Weight range** — clamped to `[0.0, 1.0]`.
6. **Title length** — max 256 UTF-8 chars.
7. **Tags** — max 32 tags per node, max 64 chars per tag, lowercase alphanumeric + hyphens only.

---

## Testing Strategy

### Unit Tests

- CRUD operations for nodes and edges (create, read, update, soft delete)
- Index consistency (nodes_by_kind, edges_by_from/to stay in sync after mutations)
- Filter combinations (kind + tag + source + time range)
- Batch operations (insert 1000 nodes in one transaction, verify all indexed)
- Edge validation (self-edge rejected, missing node rejected, duplicate detection)
- Serialization round-trip (every field survives encode → decode)
- Tombstone behaviour (deleted nodes excluded from queries by default, included with flag)
- Concurrent reads (spawn 10 reader threads while writer is inserting)

### Property Tests (proptest)

- Arbitrary node/edge generation → serialization round-trip always succeeds
- Any valid NodeFilter never panics
- Stats always consistent with actual data after any sequence of mutations

### Benchmarks (criterion)

- Single node insert latency (target: <100μs)
- Batch insert 10k nodes (target: <500ms)
- Node lookup by ID (target: <10μs)
- Filter query by kind with 100k nodes (target: <50ms)
- Edge traversal: all edges from a node with 1000 outgoing edges (target: <5ms)

---

## Deliverables

1. `cortex-core` crate with `Storage` trait and `RedbStorage` implementation
2. Complete data model with serde serialization
3. All secondary indexes maintained automatically
4. Unit tests passing with >90% coverage on storage module
5. Benchmark baselines established
6. `examples/basic_usage.rs` demonstrating CRUD and filtering
