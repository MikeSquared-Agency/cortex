# Prompt System

Cortex includes a graph-native prompt management system. Prompts are versioned, branchable, inheritable, and context-aware — all stored as nodes and edges in the same graph as your knowledge.

## Prompt Nodes

A prompt is a node of kind `prompt`. Its body stores structured content as JSON:

| Field | Description |
|-------|-------------|
| `slug` | Unique identifier (e.g. `helpful-assistant`) |
| `type` | Prompt type: `system`, `user`, `tool`, etc. |
| `branch` | Branch name (default: `main`) |
| `version` | Auto-incrementing version number |
| `sections` | Named content sections (e.g. `identity`, `constraints`, `tools`) |
| `metadata` | Arbitrary key-value pairs (including `context_weights`) |
| `override_sections` | Sections that override parent values during inheritance |

## Versioning

Each prompt slug has a linear version chain per branch. Creating a new version adds a `supersedes` edge from the previous HEAD:

```
v1 ──[supersedes]──► v2 ──[supersedes]──► v3 (HEAD)
```

The HEAD is resolved by finding the node with no incoming `supersedes` edge on that branch.

## Branches

Fork a prompt to experiment without affecting the main line:

```
main:  v1 ──► v2 ──► v3
                 │
experiment:      └──[branched_from]──► v1 ──► v2
```

Branches share no state after the fork point.

## Inheritance

A child prompt can inherit from a parent via an `inherits_from` edge. When resolved:

1. Parent sections are loaded first
2. Child sections are merged on top
3. Child's `override_sections` replace (not merge) matching parent sections

This enables a base "personality" prompt with specialised children:

```
base-agent ◄──[inherits_from]── coding-agent
base-agent ◄──[inherits_from]── writing-agent
```

## Context-Aware Selection

Variants can declare `context_weights` in their metadata — a map of signal names to weights:

```json
{
  "context_weights": {
    "user_frustrated": 0.9,
    "task_coding": 0.8,
    "energy_high": -0.3
  }
}
```

### Context Signals

The following signals are extracted from the current conversation:

| Signal | Range | Description |
|--------|-------|-------------|
| `user_pleased` / `sentiment_high` | 0.0–1.0 | User sentiment |
| `user_frustrated` | 0.0–1.0 | Inverse of sentiment |
| `correction_rate_high` | 0.0–1.0 | Rolling correction rate |
| `topic_shift_high` | 0.0–1.0 | Semantic drift from conversation start |
| `energy_high` | 0.0–1.0 | User energy proxy |
| `task_coding` | 0 or 1 | Active when task_type = coding |
| `task_planning` | 0 or 1 | Active when task_type = planning |
| `task_casual` | 0 or 1 | Active when task_type = casual |
| `task_crisis` | 0 or 1 | Active when task_type = crisis |
| `task_reflection` | 0 or 1 | Active when task_type = reflection |

### Scoring

Each variant's total score blends historical performance with context fit (50/50):

```
context_fit = dot(context_weights, signals) / sum(|weights|)
total_score = 0.5 × edge_weight + 0.5 × context_fit
```

Variants without `context_weights` use their edge weight directly.

### Epsilon-Greedy Selection

Selection uses epsilon-greedy strategy:
- With probability `1 - ε`: pick the highest-scoring variant (exploit)
- With probability `ε`: pick a random variant (explore)

Default ε = 0.2 (20% exploration).

## Observation Scoring

After each interaction, record an observation to update the variant's performance:

```
observation_score = 0.5 × sentiment
                  + 0.3 × (1 - correction_penalty)
                  + 0.2 × task_success
```

Where:
- `correction_penalty` = min(correction_count × 0.1, 1.0)
- `task_success` = 1.0 (success), 0.5 (partial), 0.0 (failure/unknown)

## EMA Edge Weight Updates

Edge weights are updated using exponential moving average with α = 0.1:

```
new_weight = 0.9 × old_weight + 0.1 × observation_score
```

This provides slow, stable adaptation. After ~22 observations from a 0.5 starting weight with perfect scores, the weight converges to ~0.9.

## Auto-Rollback

When performance degrades (observation scores drop below a configurable σ threshold), Cortex can automatically roll back to the previous version by:

1. Detecting sustained poor performance via statistical thresholds
2. Creating a `rolled_back` edge from the degraded version
3. Restoring the previous HEAD as the active version

## Migration Files

Bulk-import prompts using JSON migration files:

```json
[
  {
    "slug": "my-prompt",
    "type": "system",
    "branch": "main",
    "sections": {
      "identity": "You are helpful.",
      "constraints": "Be concise."
    },
    "metadata": {
      "context_weights": {
        "task_coding": 0.8,
        "user_frustrated": 0.5
      }
    },
    "tags": ["assistant"]
  }
]
```

Import with:

```bash
cortex prompt migrate prompts.json --dry-run  # preview
cortex prompt migrate prompts.json             # import
```
