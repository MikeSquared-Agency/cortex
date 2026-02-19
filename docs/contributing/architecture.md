# Internal Architecture (For Contributors)

## cortex-core Module Map

```
cortex-core/src/
  lib.rs              — Public API re-exports
  api.rs              — Cortex::open() library entry point
  error.rs            — CortexError enum
  kinds.rs            — NodeKind newtype + built-in constants
  relations.rs        — Relation newtype + built-in constants
  ingest.rs           — IngestAdapter trait

  storage/
    mod.rs            — Storage trait
    redb_storage.rs   — RedbStorage implementation
    encrypted.rs      — AES-256-GCM file encryption

  graph/
    engine.rs         — GraphEngine trait + GraphEngineImpl
    types.rs          — Node, Edge, Subgraph, TraversalRequest, etc.

  vector/
    embedding.rs      — EmbeddingService trait + FastEmbedService
    index.rs          — VectorIndex trait + HnswIndex + RwLockVectorIndex

  auto_linker/
    mod.rs            — AutoLinker struct + run_cycle()
    rules.rs          — SimilarityRule, DedupScanner, etc.

  briefing/
    mod.rs            — Briefing, BriefingSection types
    engine.rs         — BriefingEngine<S, E, V, G>
    cache.rs          — BriefingCache (invalidated by graph_version)
    renderer.rs       — Section renderers
    ingest.rs         — FileIngest file watcher

  policies/
    retention.rs      — RetentionEngine + sweep()
    audit.rs          — AuditLog + AuditEntry
```

## Key Design Decisions

### NodeKind and Relation as Validated Newtypes

`NodeKind` and `Relation` are `String` newtypes with validation, not enums. This allows user-defined kinds and relations without code changes. Built-in constants are provided in `kinds::defaults` and `relations::defaults`.

### Generic Storage/Graph/Vector

`BriefingEngine<S, E, V, G>` and `AutoLinker<S, E, V, G>` are fully generic over storage, embedding, vector index, and graph engine. This enables testing with mock implementations and future swapping of backends.

### Arc Types for Non-Clone Services

`FastEmbedService`, `HnswIndex`, and `GraphEngineImpl` don't implement `Clone`. Blanket impls of `EmbeddingService` and `GraphEngine` for `Arc<T>` let callers pass `Arc<T>` where a `T` is expected.

### graph_version AtomicU64

An `Arc<AtomicU64>` is shared across gRPC, HTTP, NATS, and the auto-linker. Every mutation increments it. The briefing cache uses it as a cheap invalidation signal.

### Result Alias Shadowing

`use cortex_core::*` imports `type Result<T> = cortex_core::Result<T>`. Files that also need `std::result::Result<T, E>` (two type params) must add `use std::result::Result;`.
