# Python SDK Reference

## Install

```bash
pip install cortex-memory
```

## Quick Start

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
```

## Cortex(addr)

Connect to a Cortex server.

- `addr` — gRPC address, e.g. `"localhost:9090"`

## cx.store(kind, title, *, body="", importance=0.5, tags=None, source_agent="", metadata=None) → str

Store a new node. Returns the node ID.

```python
node_id = cx.store("fact", "JWT is used for auth",
                   body="We chose JWT for stateless...",
                   importance=0.8,
                   tags=["auth", "security"])
```

## cx.get(node_id) → Node

Get a node by ID.

## cx.search(query, limit=10, *, kind=None, hybrid=False, alpha=0.7) → List[SearchResult]

Search nodes semantically.

```python
results = cx.search("authentication", limit=5)
for r in results:
    print(r.score, r.title, r.id)
```

## cx.briefing(agent_id, *, max_tokens=2000) → str

Get a context briefing for an agent.

```python
briefing = cx.briefing("my-agent")
# Use as system prompt context
```

## cx.edge(from_id, to_id, relation, *, weight=0.8) → str

Create an edge between two nodes.

```python
cx.edge(node_a, node_b, "supports", weight=0.9)
```

## cx.delete(node_id) → None

Delete a node.

## cx.traverse(node_id, *, depth=2, direction="both") → Subgraph

Traverse the graph from a node.

## SearchResult

| Field | Type | Description |
|-------|------|-------------|
| `id` | str | Node ID |
| `title` | str | Node title |
| `body` | str | Node body |
| `kind` | str | Node kind |
| `score` | float | Similarity score |
| `importance` | float | Importance score |

## Node

| Field | Type | Description |
|-------|------|-------------|
| `id` | str | Node ID |
| `kind` | str | Node kind |
| `title` | str | Title |
| `body` | str | Body |
| `importance` | float | Importance score |
| `tags` | List[str] | Tags |
| `source_agent` | str | Source agent ID |
| `created_at` | datetime | Creation timestamp |
| `metadata` | Dict[str, str] | Metadata |
