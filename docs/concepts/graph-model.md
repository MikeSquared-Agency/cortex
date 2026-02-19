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
| `observation` | An inferred conclusion from evidence |
| `pattern` | A recurring behaviour or structure |
| `preference` | A stated preference |
| `agent` | An agent identity node |

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

Custom relations are allowed — any lowercase alphanumeric string with underscores.

## Decay

Edge weights decay over time when edges are not accessed. This models the natural fading of relevance: knowledge that hasn't been touched recently becomes less strongly connected. The auto-linker reinforces edges that remain relevant by re-observing similarity.

Decay is configurable per-relation type.
