# Phase 3 — Vector Layer: Embeddings & Similarity

**Duration:** 1 week  
**Crate:** `cortex-core` (extends Phases 1-2)  
**Dependencies:** Phase 1 complete (Phase 2 not strictly required)  
**New deps:** fastembed-rs, instant-distance (or usearch)

---

## Objective

Add vector embedding generation and similarity search to the graph. Every node gets an embedding computed from its content. Nodes can be found by semantic similarity, not just structure. This is the bridge between "I know the exact thing I'm looking for" (graph traversal) and "I know roughly what I'm looking for" (semantic search).

---

## Embedding Generation

### Model: FastEmbed

FastEmbed-rs runs ONNX models locally. No API calls. No network dependency. No token costs. No rate limits.

**Default model:** `BAAI/bge-small-en-v1.5`
- 384 dimensions
- ~33M parameters
- Fast inference (~1ms per embedding on CPU)
- Strong performance on retrieval benchmarks for its size
- English-focused (sufficient for our use case; multilingual model available if needed)

**Why not API-based embeddings (OpenAI, Voyage)?**
- Network dependency = another failure mode
- Cost per embedding = scales linearly with graph size
- Rate limits = bottleneck during bulk ingest
- We're computing embeddings for every node and recomputing on updates. Local is the only sane choice.

### Embedding Content

What gets embedded isn't just the body text. The embedding input is a structured concatenation:

```rust
fn embedding_input(node: &Node) -> String {
    format!(
        "{kind}: {title}\n{body}\ntags: {tags}",
        kind = node.kind.as_str(),
        title = node.data.title,
        body = node.data.body,
        tags = node.data.tags.join(", "),
    )
}
```

Including `kind` in the embedding input means that "Decision: use Rust for Cortex" and "Fact: Cortex is written in Rust" produce related but distinct embeddings. The model learns that these are connected but different types of knowledge.

### EmbeddingService

```rust
pub trait EmbeddingService: Send + Sync {
    /// Generate embedding for a single text.
    fn embed(&self, text: &str) -> Result<Embedding>;
    
    /// Batch embedding for efficiency. FastEmbed batches internally.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>>;
    
    /// Embedding dimension for the current model.
    fn dimension(&self) -> usize;
    
    /// Model identifier string.
    fn model_name(&self) -> &str;
}

pub struct FastEmbedService {
    model: fastembed::TextEmbedding,
    model_name: String,
    dimension: usize,
}
```

### When Embeddings Are Computed

1. **On node creation** — embedding computed immediately and stored with the node.
2. **On node update** — if `title`, `body`, or `tags` changed, recompute embedding.
3. **Bulk reindex** — admin command to recompute all embeddings (model change, dimension change).
4. **Lazy fallback** — if a node has `embedding: None` (imported without embedding), compute on first similarity query that touches it.

---

## Similarity Index

### Algorithm: HNSW (Hierarchical Navigable Small World)

HNSW is the standard for approximate nearest neighbor search. Sub-linear query time, high recall (>95% at typical settings), works well up to millions of vectors.

**Library options (pure Rust):**
- `instant-distance` — simple, well-tested, pure Rust HNSW
- `usearch` — Rust bindings, more features (quantization, multiple metrics)
- `hora` — pure Rust, multiple index types

Recommend starting with `instant-distance` for simplicity, migrate to `usearch` if we need quantization at scale.

### VectorIndex

```rust
pub trait VectorIndex: Send + Sync {
    /// Add a vector with associated node ID.
    fn insert(&mut self, id: NodeId, embedding: &Embedding) -> Result<()>;
    
    /// Remove a vector.
    fn remove(&mut self, id: NodeId) -> Result<()>;
    
    /// Find the K nearest neighbors to a query vector.
    fn search(
        &self, 
        query: &Embedding, 
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>>;
    
    /// Find all vectors within a similarity threshold.
    fn search_threshold(
        &self, 
        query: &Embedding, 
        threshold: f32,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>>;
    
    /// Batch search for auto-linker efficiency.
    fn search_batch(
        &self,
        queries: &[(NodeId, Embedding)],
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<HashMap<NodeId, Vec<SimilarityResult>>>;
    
    /// Number of vectors in the index.
    fn len(&self) -> usize;
    
    /// Rebuild the index from scratch (after bulk inserts).
    fn rebuild(&mut self) -> Result<()>;
    
    /// Save index to disk.
    fn save(&self, path: &Path) -> Result<()>;
    
    /// Load index from disk.
    fn load(path: &Path) -> Result<Self> where Self: Sized;
}

pub struct SimilarityResult {
    pub node_id: NodeId,
    pub score: f32,  // Cosine similarity, 0.0 to 1.0
    pub distance: f32,  // 1.0 - score
}

pub struct VectorFilter {
    /// Only search within these node kinds.
    pub kinds: Option<Vec<NodeKind>>,
    /// Exclude these specific node IDs.
    pub exclude: Option<Vec<NodeId>>,
    /// Only include nodes from this agent.
    pub source_agent: Option<String>,
}
```

### Index Persistence

The HNSW index lives in memory for fast queries but persists to disk:
- **File:** `$CORTEX_DATA_DIR/cortex.hnsw`
- **Save frequency:** after every N inserts (default 100) or on graceful shutdown
- **Load on startup:** if file exists, load from disk. If not (first run or corruption), rebuild from redb node embeddings.
- **Rebuild safety:** the source of truth is always redb. The HNSW file is a cache. Deleting it just means a slower startup while it rebuilds.

---

## Hybrid Retrieval

The killer feature. Combine vector similarity with graph proximity for retrieval that's smarter than either alone.

### HybridQuery

```rust
pub struct HybridQuery {
    /// Text to search for semantically.
    pub query_text: String,
    
    /// Optional: bias results toward nodes connected to these anchor nodes.
    /// Graph proximity to anchors boosts ranking.
    pub anchors: Vec<NodeId>,
    
    /// How much to weight vector similarity vs graph proximity.
    /// 0.0 = pure graph, 1.0 = pure vector. Default 0.7.
    pub vector_weight: f32,
    
    /// Maximum results.
    pub limit: usize,
    
    /// Node kind filter.
    pub kind_filter: Option<Vec<NodeKind>>,
    
    /// Maximum graph distance from anchors to consider.
    /// Nodes beyond this distance get zero graph proximity score.
    pub max_anchor_depth: u32,
}

pub struct HybridResult {
    pub node: Node,
    pub vector_score: f32,    // Raw cosine similarity
    pub graph_score: f32,     // Proximity to anchors (0.0 - 1.0)
    pub combined_score: f32,  // Weighted blend
    pub nearest_anchor: Option<(NodeId, u32)>,  // Closest anchor and depth
}
```

### Scoring Algorithm

```
combined_score = (vector_weight × vector_score) + ((1 - vector_weight) × graph_score)

graph_score = max over all anchors of: 1.0 / (1.0 + shortest_path_length_to_anchor)
```

A node that's semantically relevant AND close to the anchor nodes in the graph ranks highest. A node that's semantically relevant but disconnected from the context ranks lower. This prevents hallucination-style retrieval where irrelevant-but-similar memories pollute the context.

### Example

Agent Kai is asking about infrastructure decisions. Query: "why did we choose Go for Dispatch?"

- **Anchors:** [kai_agent_node, dispatch_fact_node]
- **Vector search** finds: the decision node about Go vs Rust, a fact about Dispatch's port, a pattern about I/O-bound workloads
- **Graph proximity** boosts: the decision node (directly connected to Dispatch fact via `AppliesTo`) and the pattern (connected via `InformedBy`)
- **Result:** decision node ranks #1 because it scores high on both dimensions

---

## Similarity Thresholds

Configurable thresholds that control auto-linking behavior (used in Phase 4):

```rust
pub struct SimilarityConfig {
    /// Minimum cosine similarity to create an auto-edge.
    /// Too low = noise, too high = misses connections.
    /// Default: 0.75
    pub auto_link_threshold: f32,
    
    /// Minimum similarity to flag as potential duplicate.
    /// Default: 0.92
    pub dedup_threshold: f32,
    
    /// Minimum similarity to flag as potential contradiction.
    /// (High similarity + opposing sentiment/content)
    /// Default: 0.80
    pub contradiction_threshold: f32,
    
    /// Number of nearest neighbors to check per node
    /// during auto-linking scan.
    /// Default: 20
    pub auto_link_k: usize,
}
```

---

## Migration from Alexandria

Alexandria currently stores 54 entries as flat key-value pairs in remote Supabase with embeddings that are never queried. Migration path:

1. Pull all 54 entries via Alexandria API
2. Map categories (`fact`, `decision`, `discovery`) to NodeKinds
3. Create nodes with original content
4. Recompute embeddings locally (don't trust remote embeddings — different model, different dimensions potentially)
5. Run auto-linker to discover relationships between imported nodes
6. Verify via graph traversal that the imported knowledge forms coherent subgraphs

This is a one-time operation, scripted as `cortex migrate-alexandria`.

---

## Testing Strategy

### Unit Tests

- Embedding generation produces correct dimension vectors
- Same input → same embedding (deterministic)
- Similar texts → high cosine similarity (>0.8)
- Unrelated texts → low cosine similarity (<0.3)
- HNSW insert/search round-trip: insert 1000 vectors, search returns correct nearest neighbors
- HNSW with filter: kind filter excludes correct results
- Threshold search: returns only results above threshold
- Hybrid retrieval: anchor-connected nodes rank higher than disconnected nodes with same vector score
- Index persistence: save → load → search produces same results
- Index rebuild: delete file → rebuild from redb → same search results

### Benchmarks

- Embed single text (target: <5ms)
- Embed batch of 100 texts (target: <200ms)
- HNSW search top-10 in 10k vectors (target: <1ms)
- HNSW search top-10 in 100k vectors (target: <5ms)
- Hybrid query with 3 anchors, 10k nodes (target: <50ms)
- Full reindex of 10k nodes (target: <60s)

---

## Deliverables

1. `FastEmbedService` implementation with local model inference
2. HNSW vector index with persistence and rebuild capability
3. Hybrid retrieval combining vector similarity + graph proximity
4. Configurable similarity thresholds
5. Alexandria migration script
6. Benchmark baselines for embedding and search performance
