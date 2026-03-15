# Auto-Linker

The auto-linker is a background process that automatically discovers relationships between nodes and maintains the graph structure.

## What It Does

Every `interval_seconds` (default: 60), the auto-linker:

1. **Processes the backlog** -- newly created or modified nodes since the last cycle
2. **Similarity scan** -- for each backlog node, searches the vector index for similar nodes
3. **Edge creation** -- creates `similar_to` edges for pairs above `similarity_threshold`
4. **Structural rules** -- applies configurable rules (see below) to create typed edges
5. **Contradiction detection** -- identifies nodes that contradict existing knowledge and creates `contradicts` edges
6. **Entity co-occurrence** -- when two nodes from different agents reference the same entity, creates a `shared_entity` edge
7. **Entity promotion** -- when an entity string appears across 2+ agents, promotes it to a first-class entity node (periodic, configurable interval)
8. **Dedup** -- removes redundant or conflicting edges

## Configurable Rules

Rules define how the auto-linker creates typed edges beyond basic similarity. Each rule specifies source/target kinds, the relation to create, and a condition that must be met.

```toml
[[auto_linker.rules]]
name = "decision-leads-to-event"
from_kind = "decision"
to_kind = "event"
relation = "led_to"
weight = 0.8
condition = { type = "temporal_proximity", window_minutes = 60 }
```

### Wildcard Kinds

Use `"*"` to match any kind:

```toml
[[auto_linker.rules]]
name = "same-agent-linking"
from_kind = "*"
to_kind = "*"
relation = "related_to"
weight = 0.6
condition = { type = "same_agent" }
```

### Condition Types

| Type | Parameters | Description |
|------|-----------|-------------|
| `min_similarity` | `threshold` | Vector similarity above threshold |
| `shared_tags` | `min_shared` | Minimum shared tags between nodes |
| `temporal_proximity` | `window_minutes` | Created within N minutes of each other |
| `newer_than` | -- | Source node is newer than target |
| `same_agent` | -- | Both nodes from the same agent |
| `body_field_ref` | `field` | Body contains a reference to target's field |
| `body_field_contains` | `field`, `value` | Body field contains a specific value |
| `tag_references_title` | -- | Source has a tag matching target's title |
| `negation_detected` | -- | Source negates target's content |

### Legacy Rule Deprecation

The hardcoded structural rules (causality, temporal sequencing, support detection, reference matching) have been replaced by configurable rules. When any `[[auto_linker.rules]]` are defined in config, legacy rules are automatically disabled.

To replicate legacy behaviour, use the default rules in `cortex.example.toml`. To customise, modify or remove rules as needed.

## Entity Features

### Entity Co-occurrence

When two nodes from different agents reference the same entity (via `entity-` prefixed tags or `metadata.entities`), the auto-linker creates a `shared_entity` edge between them with the entity name in edge metadata.

### Entity Promotion

Every `entity_promote_every_n_cycles` cycles (default: 60), the auto-linker scans for entity strings referenced by `entity_promote_min_agents` or more agents (default: 2). Matching strings are promoted to first-class entity nodes with `kind: "entity"` and `metadata.entity_type`.

See [Entity Resolution](./entity-resolution.md) for the full model.

## Configuration

```toml
[auto_linker]
enabled = true
interval_seconds = 60
similarity_threshold = 0.75
max_edges_per_node = 20
entity_promote_every_n_cycles = 60
entity_promote_min_agents = 2
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
# Shows: cycles, nodes_processed, edges_created, edges_pruned, entities_promoted
```

## Similarity Threshold

The threshold controls how similar two nodes must be (cosine similarity over their embeddings) before an edge is created.

- `0.90+` -- near-identical content only
- `0.75` -- related concepts (recommended default)
- `0.60` -- broad associations

## Deduplication

The dedup scanner runs after each cycle and removes edges where:
- The same relationship is represented by multiple edges (keeps the highest-weight one)
- The source or target node has been deleted
