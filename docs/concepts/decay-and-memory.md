# Decay and Memory

Cortex has two mechanisms for handling knowledge staleness:

1. **Edge weight decay** -- relationship strength fades over time unless reinforced
2. **Temporal validity** -- nodes carry truth windows (`valid_from`/`valid_until`) and lifecycle expiry (`expires_at`)

These are distinct concerns. Decay is about *relevance* (how strongly connected). Temporal validity is about *truth* (when a fact was actually true).

## Edge Weight Decay

Edge weights start at 1.0 when created. Over time, weights decrease based on:

- **Time since last access** -- unused edges decay faster
- **Relation type** -- some relations (e.g. `fact`) decay slower than others (e.g. `similar_to`)
- **Node importance** -- edges connected to high-importance nodes decay more slowly

The decay function is exponential:

```
weight(t) = weight_0 * e^(-lambda * t)
```

Where `lambda` is the decay rate (configurable per relation) and `t` is time since last access.

### Why Decay?

Without decay, a graph accumulated over months becomes noisy. Old, irrelevant knowledge crowds out recent, relevant knowledge in briefings and search results.

Decay ensures that:
- Recent knowledge is surfaced preferentially
- Stale relationships fade away naturally
- Important knowledge (high `importance` score) persists longer

### Reinforcement

Accessing a node or edge (via search, traversal, or briefing generation) reinforces it -- resets or slows its decay. This mirrors how memory works: things you think about stay sharp, things you ignore fade.

The auto-linker also reinforces similarity edges it re-observes in each cycle.

## Temporal Validity

Nodes can declare when their content was true:

- `valid_from`: when the fact became true (None = always true)
- `valid_until`: when the fact stopped being true (None = still true)

Query with: `NodeFilter::new().valid_at(Utc::now())`

```bash
# Store a fact with temporal bounds
cortex node create --kind fact --title "Rate limit is 1000/min" \
  --valid-from 2026-01-01T00:00:00Z

# Search for facts true at a specific time
cortex search "rate limit" --valid-at 2026-01-15T00:00:00Z
```

Nodes past their `valid_until` remain in the graph for historical queries but are excluded from briefings and default search results.

## Lifecycle Expiry

`expires_at` is distinct from `valid_until`:

- `valid_until` = epistemic ("this stopped being true")
- `expires_at` = lifecycle ("delete this from the graph")

Use cases:
- **Sub-agent working memory**: expires after task completion
- **GDPR compliance**: expires on data retention deadline
- **Ephemeral context**: temporary notes that auto-clean

```bash
# Store a node that expires in 7 days
cortex node create --kind fact --title "Sprint goal: fix auth bug" \
  --expires-at 2026-03-22T00:00:00Z
```

The retention engine sweeps nodes past their `expires_at` during each cycle.

## Retention Policies

Hard retention limits are separate from decay. See [configuration](../getting-started/configuration.md) for `[retention]` settings.

```bash
# View nodes approaching expiry
cortex node list --expiring-soon

# Manually trigger a retention sweep
cortex node prune
```
