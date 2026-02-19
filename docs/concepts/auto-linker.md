# Auto-Linker

The auto-linker is a background process that automatically discovers relationships between nodes and maintains the graph structure.

## What It Does

Every `interval_seconds` (default: 60), the auto-linker:

1. **Processes the backlog** — newly created or modified nodes since the last cycle
2. **Similarity scan** — for each backlog node, searches the vector index for similar nodes
3. **Edge creation** — creates `similar_to` edges for pairs above `similarity_threshold`
4. **Dedup** — removes redundant or conflicting edges
5. **Contradiction detection** — identifies nodes that contradict existing knowledge and creates `contradicts` edges

## Configuration

```toml
[auto_linker]
enabled = true
interval_seconds = 60
similarity_threshold = 0.75
max_edges_per_node = 20
```

## Manual Trigger

```bash
# Trigger an immediate auto-linker cycle
cortex node link --trigger

# Or via HTTP
curl -X POST http://localhost:9091/auto-linker/trigger
```

## Monitoring

```bash
cortex stats
# Shows: cycles, nodes_processed, edges_created, edges_pruned
```

## Similarity Threshold

The threshold controls how similar two nodes must be (cosine similarity over their embeddings) before an edge is created.

- `0.90+` — near-identical content only
- `0.75` — related concepts (recommended default)
- `0.60` — broad associations

## Deduplication

The dedup scanner runs after each cycle and removes edges where:
- The same relationship is represented by multiple edges (keeps the highest-weight one)
- The source or target node has been deleted
