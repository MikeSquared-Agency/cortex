# Cortex Navigation Skill

How to use Cortex as an agent — read knowledge, write knowledge, search the graph, get briefings.

---

## First Thing You Do

At the start of every session, call:

```
cortex_briefing(agent_id="YOUR_AGENT_ID", compact=true)
```

This returns your pending goals, recent decisions, and anything flagged for your attention. Do not skip this.

---

## MCP Tools (Primary Interface)

MCP is the preferred way to interact with Cortex. Use HTTP only when MCP doesn't cover your need.

### cortex_store

Store a new knowledge node.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `title` | string | yes | Descriptive, >= 10 chars |
| `body` | string | no | Detailed context, >= 20 chars |
| `kind` | string | no | `fact`, `decision`, `goal`, `event`, `pattern`, `observation`, or any valid kind |
| `importance` | float | no | 0.0–1.0 (see scoring guide below) |
| `tags` | string[] | no | Required when importance >= 0.7 |

```
cortex_store(
  title="Decided to use redb for Cortex storage layer",
  body="We decided to use redb over SQLite and Postgres because it provides zero-copy mmap reads, ACID transactions, and no network dependencies — critical for an embedded graph engine.",
  kind="decision",
  importance=0.8,
  tags=["architecture", "storage", "redb"]
)
```

**When to use:** After making a decision, learning a fact, completing a goal, observing a pattern, or any time you produce knowledge worth persisting.

### cortex_search

Semantic search — finds nodes by meaning.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `query` | string | yes | Natural language query |
| `limit` | int | no | Max results to return |

```
cortex_search(query="database selection rationale", limit=5)
```

**When to use:** When you need to find knowledge about a topic but don't know exact node IDs.

### cortex_recall

Hybrid search — combines vector similarity with graph proximity. Returns richer results than `cortex_search` by factoring in how nodes are connected.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `query` | string | yes | Natural language query |
| `limit` | int | no | Max results to return |

```
cortex_recall(query="infrastructure decisions and their outcomes", limit=10)
```

**When to use:** When you want context-aware results that include related nodes, not just direct matches. Prefer this over `cortex_search` for broad research.

### cortex_briefing

Get a summary of what matters to you right now.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `agent_id` | string | no | Your agent identifier |
| `compact` | bool | no | `true` for a shorter summary |

```
cortex_briefing(agent_id="YOUR_AGENT_ID", compact=true)
```

**When to use:** Start of every session. Also useful mid-session if you've been working for a while and want to re-orient.

### cortex_traverse

Walk the graph from a specific node to explore its connections.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `node_id` | string | yes | Starting node ID |
| `depth` | int | no | How many hops to traverse |
| `direction` | string | no | Direction to walk |

```
cortex_traverse(node_id="abc123", depth=2, direction="outgoing")
```

**When to use:** When you found a relevant node and want to see what it connects to — related decisions, goals, patterns, etc.

### cortex_relate

Create an edge between two nodes.

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `from_id` | string | yes | Source node ID |
| `to_id` | string | yes | Target node ID |
| `relation` | string | no | Relation type (see common relations below) |

```
cortex_relate(from_id="abc123", to_id="def456", relation="supersedes")
```

**When to use:** After storing a new node that relates to existing knowledge. Always wire edges — don't leave nodes orphaned.

**Common relations:** `relates_to`, `similar_to`, `contradicts`, `supersedes`, `has_step`, `has_tool`, `applies_to`, `reply_to`, `authored_by`, `informed_by`. Use lowercase with underscores.

---

## How Cortex Works

**Nodes** are the atomic unit of knowledge. Each has a kind, title, body, importance score, tags, and metadata. Nodes are identified by UUID.

**Edges** connect nodes with typed relations. Edges have a source (who created them) and optional provenance metadata.

**Embeddings** are generated automatically for every node using BAAI/bge-small-en-v1.5 (384 dimensions, local via FastEmbed). You never need to compute embeddings yourself.

**Auto-linker** runs in the background. It creates `similar_to` edges between semantically close nodes, prunes low-weight edges over time, and merges near-duplicates (>0.95 similarity). You don't need to manage this — just store good nodes and the auto-linker handles connectivity.

---

## Node Kinds

`NodeKind` is a validated string newtype, not a fixed enum. Custom kinds are valid. Common kinds:

| Kind | Use for | Example |
|------|---------|---------|
| `fact` | Confirmed, objective information | "Cortex uses redb for embedded storage" |
| `decision` | Choices made with rationale (must contain action language) | "Decided to use gRPC for production API" |
| `goal` | Desired outcomes, targets | "Ship Cortex v1.0 by end of Q2" |
| `event` | Things that happened at a specific time | "Cortex v0.9 released on 2025-03-15" |
| `pattern` | Recurring behaviors (must reference recurrence) | "Users always search before creating nodes" |
| `observation` | Subjective or uncertain assessments | "The auto-linker seems slow on graphs > 10k nodes" |
| `agent` | Agent identity / capabilities | "Kai — infrastructure automation agent" |
| `workflow` | Multi-step processes | "Deploy pipeline: build, test, stage, prod" |
| `tool` | Tool descriptions / capabilities | "cortex_search — semantic search over the knowledge graph" |
| `domain` | Domain concepts / taxonomy | "Graph memory — knowledge persistence for AI agents" |
| `preference` | User or agent preferences | "Prefer compact briefings over verbose ones" |

**Caveat:** HTTP responses return kinds in PascalCase (`"Fact"`), but you send lowercase (`"fact"`) when creating nodes.

---

## Importance Scoring

| Range | Meaning | Example |
|-------|---------|---------|
| 0.0–0.3 | Low — ephemeral, context-specific, may decay | Session notes, temporary observations |
| 0.4–0.6 | Medium — useful knowledge, standard retention | Most facts, routine decisions |
| 0.7–0.8 | High — significant decisions, key patterns | Architectural decisions, critical constraints |
| 0.9–1.0 | Critical — core identity, invariants, must not be lost | Foundational goals, hard constraints |

Nodes with importance >= 0.7 require tags. Nodes with importance >= 0.8 need body >= 50 chars. Nodes with importance >= 0.9 need body >= 100 chars.

---

## The Task Loop

When executing tasks that involve Cortex knowledge, follow this pattern:

1. **Search first.** Call `cortex_recall` or `cortex_search` to see what's already known. Never create a node without checking for existing knowledge.
2. **Check for workflows.** If a relevant workflow node exists, follow its steps rather than improvising.
3. **Execute.** Do the work.
4. **Store the outcome.** Use `cortex_store` to persist what you learned, decided, or accomplished.
5. **Wire edges.** Use `cortex_relate` to connect your new node to related existing nodes. Don't leave nodes orphaned.

---

## HTTP API (Advanced / Direct Access)

Use HTTP when MCP tools don't cover your need — bulk operations, filtering, updates, deletes.

Server runs on `http://localhost:9091` by default.

### Reading

| Method | Endpoint | Query Params | Purpose |
|--------|----------|-------------|---------|
| GET | `/health` | — | Health check |
| GET | `/stats` | — | Graph statistics |
| GET | `/metrics` | — | Prometheus metrics |
| GET | `/nodes` | `kind`, `agent`, `tag`, `limit`, `offset` | List/filter nodes |
| GET | `/nodes/:id` | — | Get single node |
| GET | `/nodes/:id/neighbors` | — | Get connected nodes |
| GET | `/edges/:id` | — | Get single edge |
| GET | `/search` | `q`, `limit`, `kind` | Semantic search |
| GET | `/search/hybrid` | `q`, `limit` | Hybrid search (vector + graph) |
| GET | `/viz` | — | D3 graph visualisation |
| GET | `/graph/export` | — | Export full graph |
| GET | `/auto-linker/status` | — | Auto-linker state |
| GET | `/briefing/:agent_id` | — | Agent briefing |
| GET | `/events/stream` | `events` | SSE real-time graph change stream |

### Writing

**Create a node:**

```
POST /nodes?gate=skip
Content-Type: application/json
x-gate-override: true
x-agent-id: YOUR_AGENT_ID

{
  "title": "Node title here (>= 10 chars)",
  "body": "Detailed context (>= 20 chars)",
  "kind": "fact",
  "importance": 0.5,
  "tags": ["relevant", "tags"]
}
```

The `?gate=skip` and `x-gate-override: true` bypass the write gate. Omit both to let the gate validate your node (recommended unless you have a reason to bypass).

The `x-agent-id` header is used for audit logging. Always include it.

**Update a node:**

```
PATCH /nodes/:id
Content-Type: application/json
x-agent-id: YOUR_AGENT_ID

{
  "title": "Updated title",
  "body": "Updated body"
}
```

**Delete a node:**

```
DELETE /nodes/:id
x-agent-id: YOUR_AGENT_ID
```

**Create an edge:**

```
POST /edges
Content-Type: application/json
x-agent-id: YOUR_AGENT_ID

{
  "from_id": "source-node-uuid",
  "to_id": "target-node-uuid",
  "relation": "relates_to"
}
```

**Trigger auto-linker:**

```
POST /auto-linker/trigger
```

**Note:** The `body` field in HTTP JSON responses is a JSON string. Parse it with `JSON.parse()` (or equivalent) if you need the raw text.

---

## Write Gate

The write gate validates nodes before accepting them. It runs four checks in order. If any check fails, the node is rejected with a 422 response containing the reason and a suggestion.

### Check 1: Substance

Is this worth storing?

- Title must be >= 10 characters
- Body must be >= 20 characters (configurable per kind)
- Body cannot be identical to title
- Body cannot be a bare URL, a single word, or just a timestamp
- **Decision** nodes must contain action language (`decided`, `chose`, `will`, `should`, `use`, `adopt`, `switch`, `selected`, `going to`, `opted`)
- **Fact** nodes must not start with hedging (`I think`, `maybe`, `probably`) — use `observation` kind instead
- **Pattern** nodes must reference recurrence (`when`, `always`, `never`, `tends to`, `pattern`, `recurring`, `consistently`, `typically`, `usually`)

### Check 2: Specificity

Is this useful as a standalone record?

- Body must not start with unresolved pronouns (`He`, `She`, `They`, `It`) unless the title names the referent
- Title and opening body must not use unanchored relative time references (`yesterday`, `last week`, etc.) — use specific dates
- Importance >= 0.8 requires body >= 50 chars
- Importance >= 0.9 requires body >= 100 chars
- Importance >= 0.7 requires at least one tag

### Check 3: Conflict

Does this duplicate or contradict existing knowledge?

- Cosine similarity > 0.92 with an existing node = **duplicate rejection** (always, regardless of kind/agent)
- Cosine similarity > 0.85 with same kind + same agent = **near-duplicate rejection**
- Cosine similarity > 0.85 with same kind + different agent = **contradiction flag**

### Check 4: Schema

Does this node's metadata satisfy per-kind schema constraints?

- If a `[schemas.<kind>]` is configured, all required_fields must be present in metadata
- Field types must match (string, number, boolean, array)
- Numeric fields must satisfy min/max constraints
- String fields must be in `allowed_values` if defined
- Kinds without schemas pass freely

### Bypassing the Gate

To bypass all three checks, include both:
- Query param: `?gate=skip`
- Header: `x-gate-override: true`

Both are required. Only bypass when you have a legitimate reason (e.g., bulk migration, testing). In normal operation, let the gate do its job — it keeps the graph clean.

---

## Query DSL

Cortex supports a string-based filter DSL that compiles to `NodeFilter`. Use it with the `GET /nodes` endpoint's `filter` parameter or programmatically via `cortex_core::parse_filter()`.

### Syntax

```text
kind:decision AND importance>0.7
kind:fact AND agent:kai AND tags:backend,rust
(kind:decision OR kind:pattern) AND tags:architecture
created_after:7d AND kind:fact
NOT deleted:true
kind:fact AND limit:10
```

### Supported Fields

| Field | Operators | Example |
|-------|----------|---------|
| `kind` | `:` | `kind:decision`, `kind:fact,decision` |
| `tags` | `:` | `tags:backend,rust` |
| `agent` | `:` | `agent:kai` |
| `importance` | `>`, `>=`, `=` | `importance>=0.7` |
| `created_after` | `:` | `created_after:7d`, `created_after:24h` |
| `created_before` | `:` | `created_before:30d` |
| `deleted` | `:` | `deleted:true` |
| `limit` | `:` | `limit:10` |

### Logical Operators

- `AND` — both conditions must match
- `OR` — either condition matches (same field type only)
- `NOT` — negation (only for `deleted` field)
- Parentheses for grouping: `(kind:fact OR kind:decision) AND tags:arch`

---

## Schema Validation

Per-kind metadata schemas are defined in `cortex.toml` under `[schemas.*]`. When a schema is active for a kind, the write gate validates node metadata at write time (both HTTP and gRPC).

```toml
[schemas.decision]
required_fields = ["rationale"]

[schemas.decision.fields.rationale]
type = "string"

[schemas.decision.fields.priority]
type = "number"
min = 1.0
max = 5.0
```

If validation fails, the write gate rejects the node with a 422 response containing `gate.check == "schema"` and specific violation details.

Pass metadata when creating nodes:

```json
POST /nodes
{
  "kind": "decision",
  "title": "Use redb for storage layer",
  "body": "We decided to use redb for its zero-copy mmap design...",
  "metadata": {
    "rationale": "Zero-copy mmap, ACID, no network deps",
    "priority": 3
  }
}
```

---

## Anti-Patterns

- **Don't skip search.** Always check what exists before creating. Duplicates waste graph space and confuse future searches.
- **Don't orphan nodes.** Every node you create should connect to at least one other node via `cortex_relate`. Isolated nodes are invisible to graph traversal.
- **Don't store noise.** Session ephemera, debug logs, raw API responses — these don't belong in the graph. Store conclusions, not raw data.
- **Don't hardcode node IDs.** IDs are UUIDs. Always discover them via search or traversal.
- **Don't use vague titles.** "Update" or "Note" tells you nothing in six months. Be specific: "Decided to migrate auth service to OAuth2".
- **Don't assume kinds are fixed.** `NodeKind` is a validated string, not an enum. Standard kinds exist but custom kinds are valid.
- **Don't ignore the gate.** If the write gate rejects your node, fix the node — don't reflexively bypass. The gate catches real problems.

---

## CLI Reference

For terminal-based agents or manual use:

```bash
cortex search "your query"              # Semantic search
cortex briefing YOUR_AGENT_ID           # Agent briefing
cortex node create --kind fact --title "..." --body "..."  # Create node
cortex node get <id>                    # Read a node
cortex shell                            # Interactive REPL
```

---

## Quick Reference Table

| What you want | Tool / Endpoint |
|---------------|----------------|
| Get oriented at session start | `cortex_briefing` |
| Find knowledge by meaning | `cortex_search` |
| Find knowledge with graph context | `cortex_recall` |
| Store new knowledge | `cortex_store` |
| Connect two nodes | `cortex_relate` |
| Explore a node's neighborhood | `cortex_traverse` |
| List/filter nodes | `GET /nodes?kind=fact&limit=10` |
| Update a node | `PATCH /nodes/:id` |
| Delete a node | `DELETE /nodes/:id` |
| Export the full graph | `GET /graph/export` |
| Visualise the graph | `GET /viz` |
| Check system health | `GET /health` |
| Filter nodes with DSL | `parse_filter("kind:fact AND importance>0.7")` |
| Stream real-time changes | `GET /events/stream` |
