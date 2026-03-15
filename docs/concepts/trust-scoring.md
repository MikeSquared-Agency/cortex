# Trust Scoring

Trust in Cortex is not a field you set. It is computed from the graph structure at query time, like PageRank derives authority from link structure. A node's trust score reflects how well-corroborated, how uncontested, and how actively used that knowledge is across the entire graph.

## Five Signals

Trust is composed of five independent signals, each capturing a different dimension of reliability.

### 1. Corroboration

How many independent agents stored similar facts. More independent sources means higher trust. Corroboration saturates at `corroboration_saturation` (default: 3) agents -- the marginal value of additional sources diminishes. Only edges with weight >= `corroboration_min_weight` (default: 0.6) count. Weak similarity edges are excluded.

### 2. Contradiction Penalty

Unresolved `contradicts` edges reduce trust. Each active contradiction applies a penalty scaled by `contradiction_weight` (default: 0.3). Resolved contradictions (where one side is superseded) do not count. This incentivizes agents to resolve conflicts rather than leaving them open.

### 3. Source Track Record

Historical accuracy of the authoring agent. Computed from the ratio of corroborated to contradicted nodes for that agent across the entire graph. An agent whose facts are frequently corroborated earns a high source score; one whose facts are frequently contradicted earns a low one. Cached per-agent, refreshed every N auto-linker cycles.

### 4. Access Reinforcement

Frequently retrieved nodes that are never corrected gain trust. Each access (search hit, briefing inclusion, explicit retrieval) increments the counter. Saturates at `access_saturation` (default: 20) accesses. The intuition: if agents keep retrieving a node and never contradict it, it is probably accurate.

### 5. Freshness

Recently created or accessed nodes receive a freshness boost. Exponential decay with `freshness_halflife` (default: 90 days). Prevents the graph from being dominated by old, possibly outdated knowledge even when that knowledge has high corroboration.

## Combination Formula

The five signals are combined as a weighted sum. Each signal is normalized to 0.0-1.0 before weighting:

```
trust = 0.30 * corroboration
      + 0.25 * (1 - contradiction_penalty)
      + 0.20 * source_track_record
      + 0.15 * access_reinforcement
      + 0.10 * freshness_boost
```

The weights must sum to 1.0. Override them in `[trust.weights]`.

## Configuration

```toml
[trust]
corroboration_saturation = 3       # Max agents before corroboration maxes out
corroboration_min_weight = 0.6     # Minimum edge weight to count as corroboration
contradiction_weight = 0.3         # Penalty per unresolved contradiction
access_saturation = 20             # Access count at which reinforcement maxes out
freshness_halflife = 90            # Days until freshness signal halves

[trust.weights]
corroboration = 0.30
contradiction = 0.25
source = 0.20
access = 0.15
freshness = 0.10
```

All values have sensible defaults. You only need to add `[trust]` to your `cortex.toml` if you want to override them.

## API

| Interface | Command / Endpoint | Description |
|-----------|-------------------|-------------|
| CLI | `cortex trust <node-id>` | Trust score for a single node |
| CLI | `cortex trust --agent <agent-id>` | Aggregate trust metrics for an agent |
| HTTP | `GET /trust/:node_id` | Single node trust score |
| HTTP | `POST /trust/batch` | Batch scores (JSON array of node IDs) |
| HTTP | `GET /trust/agents` | Trust summary per agent |
| gRPC | `GetTrustScore` | Single node trust score |
| gRPC | `BatchGetTrustScore` | Batch trust scores |

## Trust in Briefings

When trust scoring is enabled, the briefing engine uses trust to rank and annotate nodes. Briefing nodes are ranked by a blended score:

```
briefing_rank = 0.6 * importance + 0.4 * trust_score
```

Nodes with unresolved contradictions are flagged in the Unresolved section of the briefing, regardless of their trust score. This ensures agents are always aware of contested knowledge.

## Design Decision: Computed, Not Stored

Trust is deliberately not stored as a field on nodes. Stored confidence goes stale the moment the graph changes. A new corroborating edge from another agent should immediately increase trust. A new contradiction should immediately decrease it.

By computing trust at query time from the live graph topology, Cortex ensures that trust scores always reflect the current state of knowledge. There is no migration when the trust formula changes. There is no drift between stored scores and reality. The graph is the source of truth, and trust is a view over it.
