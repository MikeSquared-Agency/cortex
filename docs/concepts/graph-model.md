# Graph Model

Cortex models knowledge as a directed property graph. The two primitive types are **nodes** and **edges**.

## Nodes

A node represents a discrete piece of knowledge.

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID v7 | Unique identifier (sortable by creation time) |
| `kind` | NodeKind | Semantic type (see below) |
| `title` | string | Short summary (indexed for search) |
| `body` | string | Full content (optional) |
| `importance` | f32 0–1 | How important this knowledge is |
| `tags` | list[string] | Free-form labels |
| `source.agent` | string | Which agent created this node |
| `created_at` | timestamp | Creation time |
| `metadata` | map | Arbitrary key-value pairs |

### Node Kinds

Node kinds are validated lowercase strings. Built-in kinds:

| Kind | Purpose |
|------|---------|
| `fact` | A stated truth or piece of information |
| `decision` | A choice that was made, with rationale |
| `event` | Something that happened |
| `goal` | An objective the agent is working toward |
| `observation` | A performance observation recording interaction outcomes |
| `pattern` | A recurring behaviour or structure |
| `preference` | A stated preference |
| `agent` | An agent identity node |
| `prompt` | A versioned prompt template with sections and metadata |

Custom kinds are allowed — any lowercase alphanumeric string with hyphens (e.g. `project-milestone`).

## Edges

An edge represents a typed, weighted relationship between two nodes.

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID v7 | Unique identifier |
| `from` | NodeId | Source node |
| `to` | NodeId | Target node |
| `relation` | Relation | Relationship type (see below) |
| `weight` | f32 0–1 | Relationship strength |
| `created_at` | timestamp | Creation time |

### Relations

Relations are validated lowercase strings with underscores. Built-in relations:

| Relation | Meaning |
|----------|---------|
| `similar_to` | High embedding similarity (auto-linked) |
| `caused_by` | Causal relationship |
| `supports` | One piece of knowledge supports another |
| `contradicts` | Knowledge conflict detected |
| `relates_to` | General association |
| `part_of` | Hierarchical containment |
| `depends_on` | Dependency relationship |
| `supersedes` | Newer version replaces older version (prompt versioning) |
| `branched_from` | Branch fork point (prompt branching) |
| `inherits_from` | Child prompt inherits parent sections |
| `uses` | Agent uses/is-bound-to a prompt variant |
| `used_by` | Reverse of uses (prompt is used by agent) |
| `performed` | Agent performed an observation |
| `informed_by` | Observation was informed by a prompt variant |
| `rolled_back` | Version was rolled back due to degradation |
| `rolled_back_to` | Target version of a rollback |

Custom relations are allowed — any lowercase alphanumeric string with underscores.

## Decay

Edge weights decay over time when edges are not accessed. This models the natural fading of relevance: knowledge that hasn't been touched recently becomes less strongly connected. The auto-linker reinforces edges that remain relevant by re-observing similarity.

Decay is configurable per-relation type.

## Prompt Graph

Cortex models prompts as first-class graph citizens. Each prompt version is a node of kind `prompt`, connected via typed edges:

```
prompt v1 ──[supersedes]──► prompt v2 ──[supersedes]──► prompt v3 (HEAD)
                                          │
                                          ├──[branched_from]──► prompt v1 (experiment branch)
                                          │
child-prompt ──[inherits_from]──► parent-prompt
```

### Versioning

Each new version creates a `supersedes` edge from the old HEAD to the new node. The HEAD is always the node with no incoming `supersedes` edge on its branch.

### Branches

Forking creates a `branched_from` edge. Each branch has its own independent supersedes chain.

### Inheritance

A child prompt linked via `inherits_from` merges its parent's sections, with the child's `override_sections` taking precedence.

### Observations

Performance observations are `observation` nodes linked via:
- `performed`: agent → observation (the agent produced this observation)
- `informed_by`: observation → prompt variant (this observation evaluated that variant)

### Agent Bindings

Agents connect to prompts via `uses` edges. The edge weight represents historical performance and is updated via exponential moving average after each observation.

For full details on the prompt system, see [Prompt System](./prompt-system.md).
