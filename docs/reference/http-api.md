# HTTP API Reference

The HTTP API is served on port 9091. It provides a REST interface for inspection and debugging.

## GET /health

Returns server health status.

```json
{
  "healthy": true,
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "stats": {
    "node_count": 1234,
    "edge_count": 5678
  }
}
```

## GET /stats

Returns node and edge counts.

## GET /nodes

List nodes with optional filtering.

Query params: `kind`, `agent`, `limit` (default 50), `offset`.

## GET /nodes/:id

Get a node by ID.

## GET /nodes/:id/neighbors

Get neighboring nodes.

Query params: `depth` (default 1), `direction` (both|outgoing|incoming).

## GET /search

Search nodes semantically.

Query params: `q` (query string, required), `limit`, `kind`.

## GET /briefing/:agent_id

Get a briefing for an agent.

## GET /graph/export

Export the full graph as JSON.

```json
{
  "success": true,
  "data": {
    "nodes": [...],
    "edges": [...]
  }
}
```

## GET /viz

Open the interactive graph visualiser in your browser.

## POST /auto-linker/trigger

Trigger an immediate auto-linker cycle.

## GET /auto-linker/status

Get auto-linker metrics.

---

## Prompt Versioning API

### GET /prompts

List the HEAD version of every prompt (grouped by slug and branch).

```bash
curl http://localhost:9091/prompts
```

### POST /prompts

Create the first version of a new prompt.

```bash
curl -X POST http://localhost:9091/prompts \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "helpful-assistant",
    "type": "system",
    "branch": "main",
    "sections": {
      "identity": "You are a helpful assistant.",
      "constraints": "Be concise and accurate."
    },
    "metadata": {},
    "author": "admin"
  }'
```

Response:

```json
{
  "success": true,
  "data": {
    "node_id": "019...",
    "slug": "helpful-assistant",
    "version": 1,
    "branch": "main"
  }
}
```

### GET /prompts/:slug/latest

Resolve the HEAD version of a prompt with inheritance applied.

Query params: `branch` (default `main`).

```bash
curl "http://localhost:9091/prompts/helpful-assistant/latest?branch=main"
```

Returns the fully resolved prompt with parent sections merged.

### GET /prompts/:slug/versions

List all versions of a prompt on a branch.

Query params: `branch` (default `main`).

```bash
curl "http://localhost:9091/prompts/helpful-assistant/versions?branch=main"
```

### GET /prompts/:slug/versions/:version

Get a specific version (raw, without inheritance resolution).

Query params: `branch` (default `main`).

```bash
curl "http://localhost:9091/prompts/helpful-assistant/versions/2?branch=main"
```

### POST /prompts/:slug/versions

Create a new version of an existing prompt.

```bash
curl -X POST http://localhost:9091/prompts/helpful-assistant/versions \
  -H "Content-Type: application/json" \
  -d '{
    "branch": "main",
    "sections": {
      "identity": "You are a helpful and concise assistant.",
      "constraints": "Be accurate. Cite sources when possible."
    },
    "author": "admin"
  }'
```

Response:

```json
{
  "success": true,
  "data": {
    "node_id": "019...",
    "slug": "helpful-assistant",
    "version": 2,
    "branch": "main"
  }
}
```

### POST /prompts/:slug/branch

Fork a prompt onto a new branch.

```bash
curl -X POST http://localhost:9091/prompts/helpful-assistant/branch \
  -H "Content-Type: application/json" \
  -d '{
    "new_branch": "experiment-v2",
    "from_branch": "main",
    "author": "admin"
  }'
```

### GET /prompts/:slug/performance

Aggregate performance metrics for a prompt variant.

Query params: `limit` (default 50).

```bash
curl "http://localhost:9091/prompts/helpful-assistant/performance?limit=20"
```

Response:

```json
{
  "success": true,
  "data": {
    "slug": "helpful-assistant",
    "prompt_id": "019...",
    "observation_count": 42,
    "avg_score": 0.78,
    "avg_sentiment": 0.82,
    "avg_correction_count": 0.5,
    "task_outcomes": {
      "success": 30,
      "partial": 8,
      "failure": 4
    },
    "observations_shown": 20,
    "observations": [...]
  }
}
```

---

## Agent Selection API

### GET /agents/:name/active-variant

Score all prompt variants bound to an agent and select the best one using epsilon-greedy selection.

Query params:

| Param | Default | Description |
|-------|---------|-------------|
| `sentiment` | `0.5` | User sentiment (0.0–1.0) |
| `task_type` | `casual` | Task type: coding, planning, casual, crisis, reflection |
| `correction_rate` | `0.0` | Correction rate (0.0–1.0) |
| `topic_shift` | `0.0` | Topic drift (0.0–1.0) |
| `energy` | `0.5` | User energy (0.0–1.0) |
| `epsilon` | `0.2` | Exploration rate (0.0–1.0) |

```bash
curl "http://localhost:9091/agents/my-agent/active-variant?sentiment=0.3&task_type=coding&epsilon=0.1"
```

Response:

```json
{
  "success": true,
  "data": {
    "agent": "my-agent",
    "selected": {
      "id": "019...",
      "slug": "coding-assistant",
      "edge_weight": 0.85,
      "context_score": 0.92,
      "total_score": 0.885
    },
    "swap_recommended": true,
    "epsilon": 0.1,
    "signals": {
      "sentiment": 0.3,
      "task_type": "coding",
      "correction_rate": 0.0,
      "topic_shift": 0.0,
      "energy": 0.5
    },
    "all_variants": [...]
  }
}
```

### GET /agents/:name/variant-history

Timeline of variant swaps and performance observations.

Query params: `limit` (default 20).

```bash
curl "http://localhost:9091/agents/my-agent/variant-history?limit=10"
```

### POST /agents/:name/observe

Record a performance observation. Updates the agent→prompt edge weight using EMA (α=0.1).

```bash
curl -X POST http://localhost:9091/agents/my-agent/observe \
  -H "Content-Type: application/json" \
  -d '{
    "variant_id": "019...",
    "variant_slug": "helpful-assistant",
    "sentiment_score": 0.8,
    "correction_count": 1,
    "task_outcome": "success",
    "token_cost": 1500
  }'
```

Response:

```json
{
  "success": true,
  "data": {
    "observation_id": "019...",
    "variant_id": "019...",
    "variant_slug": "helpful-assistant",
    "observation_score": 0.87,
    "old_edge_weight": 0.80,
    "new_edge_weight": 0.807
  }
}
```

---

## Agent Prompt Binding API

### GET /agents/:name/prompts

List all prompts bound to an agent, ordered by weight.

### PUT /agents/:name/prompts/:slug

Bind (or update) a prompt to an agent.

```bash
curl -X PUT http://localhost:9091/agents/my-agent/prompts/helpful-assistant \
  -H "Content-Type: application/json" \
  -d '{ "weight": 0.9 }'
```

### DELETE /agents/:name/prompts/:slug

Unbind a prompt from an agent.

```bash
curl -X DELETE http://localhost:9091/agents/my-agent/prompts/helpful-assistant
```

### GET /agents/:name/resolved-prompt

Get the merged effective prompt for an agent (all bound prompts combined by weight order).
