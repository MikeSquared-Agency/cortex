# Phase 4 — Auto-Linker: Self-Growing Graph

**Duration:** 1 week  
**Crate:** `cortex-core` (extends Phases 1-3)  
**Dependencies:** Phases 1-3 complete  
**New deps:** None (builds on existing embedding + graph infrastructure)

---

## Objective

Build the Cortex brain — the background process that continuously scans the graph, discovers relationships between nodes via embedding similarity and structural rules, and creates edges automatically. The graph grows itself. No human has to manually say "this decision relates to that pattern." Cortex figures it out.

This is the feature that separates a knowledge graph from a database with a graph schema.

---

## Architecture

### Processing Loop

The auto-linker runs as a background task inside the Cortex process. Not a cron job. Not triggered by a webhook. A persistent loop with configurable interval.

```
┌──────────────┐
│  Timer fires  │ (default: every 60 seconds)
└──────┬───────┘
       │
┌──────▼───────────────────┐
│  1. Scan for new/updated  │
│     nodes since last run  │
└──────┬───────────────────┘
       │
┌──────▼───────────────────┐
│  2. For each new node:    │
│     - Compute embedding   │
│       (if not already)    │
│     - Find K nearest      │
│       neighbors           │
│     - Apply link rules    │
└──────┬───────────────────┘
       │
┌──────▼───────────────────┐
│  3. Batch-create edges    │
│     in single transaction │
└──────┬───────────────────┘
       │
┌──────▼───────────────────┐
│  4. Run decay pass        │
│     (every Nth cycle)     │
└──────┬───────────────────┘
       │
┌──────▼───────────────────┐
│  5. Run dedup scan        │
│     (every Nth cycle)     │
└──────┬───────────────────┘
       │
┌──────▼───────────────────┐
│  6. Update cursor &       │
│     emit metrics          │
└──────────────────────────┘
```

### AutoLinker

```rust
pub struct AutoLinker {
    storage: Arc<dyn Storage>,
    graph: Arc<dyn GraphEngine>,
    vectors: Arc<dyn VectorIndex>,
    embeddings: Arc<dyn EmbeddingService>,
    config: AutoLinkerConfig,
    
    /// High-water mark: last processed timestamp.
    /// Persisted to redb META table so we don't reprocess after restart.
    cursor: DateTime<Utc>,
    
    /// Cycle counter for periodic tasks (decay, dedup).
    cycle_count: u64,
    
    /// Metrics for observability.
    metrics: AutoLinkerMetrics,
}

pub struct AutoLinkerConfig {
    /// How often the linker runs. Default: 60 seconds.
    pub interval: Duration,
    
    /// Similarity thresholds (from Phase 3).
    pub similarity: SimilarityConfig,
    
    /// Run decay pass every N cycles. Default: 60 (once per hour at 60s interval).
    pub decay_every_n_cycles: u64,
    
    /// Run dedup scan every N cycles. Default: 360 (every 6 hours).
    pub dedup_every_n_cycles: u64,
    
    /// Maximum nodes to process per cycle. Prevents runaway processing
    /// if there's a bulk ingest. Default: 500.
    pub max_nodes_per_cycle: usize,
    
    /// Maximum edges to create per cycle. Safety valve. Default: 2000.
    pub max_edges_per_cycle: usize,
    
    /// Whether to run on startup (process backlog). Default: true.
    pub run_on_startup: bool,
}
```

---

## Link Rules

The auto-linker applies three categories of rules to decide whether to create an edge.

### 1. Similarity Links

The primary mechanism. For each new node, find K nearest neighbors by embedding similarity. Create `RelatedTo` edges for any neighbor above the similarity threshold.

```rust
struct SimilarityLinkRule;

impl LinkRule for SimilarityLinkRule {
    fn evaluate(&self, node: &Node, neighbor: &Node, score: f32, config: &SimilarityConfig) -> Option<ProposedEdge> {
        if score >= config.auto_link_threshold {
            Some(ProposedEdge {
                from: node.id,
                to: neighbor.id,
                relation: Relation::RelatedTo,
                weight: score,  // Weight = similarity score
                provenance: EdgeProvenance::AutoSimilarity { score },
            })
        } else {
            None
        }
    }
}
```

### 2. Structural Links

Rules based on node metadata, not embeddings. These catch relationships that semantic similarity might miss.

```rust
enum StructuralRule {
    /// Same source agent → RelatedTo (weak).
    /// An agent's knowledge tends to be contextually related.
    SameAgent {
        weight: f32,  // Default: 0.3 (weak — being from same agent isn't strong evidence)
    },
    
    /// Temporal proximity → RelatedTo.
    /// Nodes created within N minutes of each other are likely contextually related.
    TemporalProximity {
        window: Duration,  // Default: 30 minutes
        weight: f32,       // Default: 0.4
    },
    
    /// Shared tags → RelatedTo.
    /// Weight scales with number of shared tags.
    SharedTags {
        min_shared: usize,  // Default: 2
        base_weight: f32,   // Default: 0.5
    },
    
    /// Decision → Event in same session → LedTo.
    /// If a decision and an event share the same session metadata,
    /// the decision likely led to the event.
    DecisionToEvent {
        weight: f32,  // Default: 0.6
    },
    
    /// Observation → Pattern with same tags → InstanceOf.
    /// An observation might be an instance of an existing pattern.
    ObservationToPattern {
        min_similarity: f32,  // Default: 0.7 (must also pass vector similarity)
        weight: f32,          // Default: 0.7
    },
    
    /// New fact supersedes old fact with same title/tags.
    /// Detects updates to existing knowledge.
    FactSupersedes {
        title_similarity: f32,  // Default: 0.9
        weight: f32,            // Default: 0.9
    },
}
```

### 3. Contradiction Detection

Special case: two nodes with high similarity but opposing content. Flagged, not auto-resolved.

```rust
struct ContradictionDetector;

impl ContradictionDetector {
    /// Check if two highly similar nodes contain contradictory information.
    /// Uses heuristics:
    /// - Negation words ("not", "never", "no longer", "stopped", "removed")
    /// - Opposing values for the same key in metadata
    /// - Temporal supersession (newer node contradicts older)
    fn check(&self, a: &Node, b: &Node, similarity: f32) -> Option<Contradiction> {
        if similarity < self.config.contradiction_threshold {
            return None;
        }
        
        // Heuristic checks...
        // If contradiction detected:
        Some(Contradiction {
            node_a: a.id,
            node_b: b.id,
            similarity,
            reason: "Negation pattern detected".into(),
            suggested_resolution: Resolution::Supersede { 
                keep: newer.id, 
                retire: older.id 
            },
        })
    }
}
```

Contradictions create a `Contradicts` edge and are surfaced in the API for human review. The auto-linker does NOT resolve contradictions automatically — it flags them. Resolution is either manual or delegated to the briefing synthesiser (Phase 6) which presents both sides.

---

## Edge Decay

Knowledge ages. Relationships weaken if not reinforced. The decay system prevents the graph from becoming an ever-growing ball of stale connections.

### Decay Algorithm

```rust
pub struct DecayConfig {
    /// Base decay rate per day. Default: 0.01 (1% per day).
    pub daily_decay_rate: f32,
    
    /// Minimum weight before an edge is pruned. Default: 0.1.
    pub prune_threshold: f32,
    
    /// Edges below this weight are candidates for deletion. Default: 0.05.
    pub delete_threshold: f32,
    
    /// Importance multiplier: high-importance nodes decay slower.
    /// effective_decay = daily_decay_rate × (1.0 - node.importance × importance_shield)
    /// Default: 0.8 (importance=1.0 node decays at 20% normal rate)
    pub importance_shield: f32,
    
    /// Access reinforcement: each access resets decay timer partially.
    /// Default: adds 7 days of "freshness"
    pub access_reinforcement_days: f32,
    
    /// Manual edges (human-created) are exempt from decay.
    pub exempt_manual: bool,  // Default: true
}

impl DecayEngine {
    fn apply_decay(&self, edge: &mut Edge, now: DateTime<Utc>) {
        // Manual edges don't decay
        if self.config.exempt_manual && matches!(edge.provenance, EdgeProvenance::Manual { .. }) {
            return;
        }
        
        let days_since_update = (now - edge.updated_at).num_seconds() as f32 / 86400.0;
        
        // Get importance of connected nodes for shielding
        let max_importance = max(
            self.get_node_importance(edge.from),
            self.get_node_importance(edge.to),
        );
        
        let effective_rate = self.config.daily_decay_rate 
            * (1.0 - max_importance * self.config.importance_shield);
        
        let decay_factor = (-effective_rate * days_since_update).exp();
        edge.weight *= decay_factor;
        
        if edge.weight < self.config.delete_threshold {
            // Mark for deletion
        } else if edge.weight < self.config.prune_threshold {
            // Mark as weak — excluded from default traversals
        }
    }
}
```

### Reinforcement

When a node is accessed (retrieved via query, included in a briefing, referenced in a conversation), its edges are reinforced:

```rust
fn reinforce(&self, node_id: NodeId) {
    // Update node access_count
    // Reset updated_at on all connected edges
    // This effectively resets the decay timer
}
```

This creates a natural selection process: frequently useful knowledge stays strong, forgotten knowledge fades.

---

## Deduplication

The dedup scanner finds near-duplicate nodes and handles them.

```rust
pub struct DedupResult {
    /// Pairs of nodes that are near-duplicates.
    pub duplicates: Vec<DuplicatePair>,
}

pub struct DuplicatePair {
    pub node_a: NodeId,
    pub node_b: NodeId,
    pub similarity: f32,
    
    /// Suggested action.
    pub suggestion: DedupAction,
}

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
```

### Merge Strategy

When merging:
1. Keep the node with more connections (higher graph centrality)
2. If tied, keep the older node (established knowledge)
3. Transfer all edges from retired node to kept node
4. Create `Supersedes` edge from kept → retired
5. Tombstone retired node (soft delete)
6. Merge metadata (union of tags, combine metadata maps)
7. Recompute embedding for kept node if body changed

---

## Metrics & Observability

```rust
pub struct AutoLinkerMetrics {
    /// Total cycles completed.
    pub cycles: u64,
    
    /// Nodes processed this cycle.
    pub nodes_processed: u64,
    
    /// Edges created this cycle.
    pub edges_created: u64,
    
    /// Edges pruned by decay this cycle.
    pub edges_pruned: u64,
    
    /// Duplicates detected this cycle.
    pub duplicates_found: u64,
    
    /// Contradictions flagged this cycle.
    pub contradictions_found: u64,
    
    /// Processing time for last cycle.
    pub last_cycle_duration: Duration,
    
    /// Current cursor position.
    pub cursor: DateTime<Utc>,
    
    /// Backlog: nodes awaiting processing.
    pub backlog_size: u64,
}
```

Exposed via the API (Phase 5) for monitoring. If `backlog_size` grows faster than processing rate, we need to increase `max_nodes_per_cycle` or decrease `interval`.

---

## Edge Cases

### Bulk Ingest

When Alexandria migration or file ingest dumps hundreds of nodes at once:
- `max_nodes_per_cycle` prevents processing them all in one cycle
- The cursor advances incrementally — backlog is processed over multiple cycles
- This is intentional: spreading the work prevents CPU spikes

### Cold Start

First run with an empty graph:
- Auto-linker has nothing to do — cursor starts at now
- As nodes are added (manually or via ingest), the linker picks them up next cycle
- First few cycles create the initial graph structure

### Restart Recovery

Cursor is persisted to redb META table. On restart:
- Load cursor → process everything since last cursor → resume normal operation
- If cursor is missing (first run or corruption) → defaults to 24 hours ago
- `run_on_startup: true` means it processes backlog immediately, doesn't wait for first timer

### Graph Explosion Prevention

A node that's similar to everything (very generic content like "infrastructure is important") would create edges to hundreds of nodes. Prevention:
- `max_edges_per_cycle` hard cap
- Per-node cap: maximum 50 auto-edges per node. If a node exceeds this, only keep the top 50 by weight.
- Generic content detection: if a node has >30 neighbors above threshold, it's probably too generic. Log a warning, don't create edges, flag for human review.

---

## Testing Strategy

### Unit Tests

- Similarity link rule creates edge above threshold, skips below
- Structural rules fire correctly (same agent, temporal proximity, shared tags)
- Contradiction detection catches negation patterns
- Decay reduces weight correctly over time
- Importance shielding reduces effective decay rate
- Access reinforcement resets decay timer
- Prune threshold removes weak edges
- Manual edges exempt from decay
- Dedup detects near-duplicates above threshold
- Merge transfers edges correctly
- Cursor persists and recovers after restart
- Max nodes/edges per cycle enforced
- Generic content detection triggers on high-fanout nodes

### Integration Tests

- Full cycle: insert 10 nodes → run linker → verify edges created
- Decay over simulated time: insert edges → advance clock → run decay → verify weights reduced
- Dedup merge: insert two near-duplicate nodes → run dedup → verify merge
- Bulk ingest: insert 1000 nodes → verify linker processes in batches across multiple cycles
- Cold start → steady state transition

### Benchmarks

- Auto-link cycle with 100 new nodes, 10k existing (target: <5s)
- Decay pass over 100k edges (target: <2s)
- Dedup scan over 10k nodes (target: <10s)

---

## Deliverables

1. `AutoLinker` with configurable processing loop
2. Similarity, structural, and contradiction link rules
3. Edge decay engine with importance shielding and access reinforcement
4. Deduplication scanner with merge strategy
5. Persistent cursor for restart recovery
6. Metrics exposure for monitoring
7. Graph explosion prevention (per-node caps, generic content detection)
