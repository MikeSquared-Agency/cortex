# Decay and Memory

Cortex models the natural fading of knowledge over time using edge weight decay.

## How Decay Works

Edge weights start at 1.0 when created. Over time, weights decrease based on:

- **Time since last access** — unused edges decay faster
- **Relation type** — some relations (e.g. `fact`) decay slower than others (e.g. `similar_to`)
- **Node importance** — edges connected to high-importance nodes decay more slowly

The decay function is exponential:

```
weight(t) = weight₀ × e^(-λt)
```

Where `λ` is the decay rate (configurable per relation) and `t` is time since last access.

## Why Decay?

Without decay, a graph accumulated over months becomes noisy. Old, irrelevant knowledge crowds out recent, relevant knowledge in briefings and search results.

Decay ensures that:
- Recent knowledge is surfaced preferentially
- Stale relationships fade away naturally
- Important knowledge (high `importance` score) persists longer

## Reinforcement

Accessing a node or edge (via search, traversal, or briefing generation) reinforces it — resets or slows its decay. This mirrors how memory works: things you think about stay sharp, things you ignore fade.

The auto-linker also reinforces similarity edges it re-observes in each cycle.

## Retention Policies

Hard retention limits are separate from decay. See [configuration](../getting-started/configuration.md) for `[retention]` settings.

```bash
# View nodes approaching expiry
cortex node list --expiring-soon

# Manually trigger a retention sweep
cortex node prune
```
