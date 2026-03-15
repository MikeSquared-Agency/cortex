# Hybrid Search

Cortex supports hybrid retrieval: combining vector similarity search with graph proximity scoring.

## How It Works

A hybrid search query runs two retrieval passes:

1. **Vector pass** — HNSW approximate nearest-neighbour search over node embeddings. Returns the top-K most semantically similar nodes.
2. **Graph pass** — For each candidate from the vector pass, traverse outgoing and incoming edges to find related nodes. Nodes reachable within N hops are scored by graph proximity.

The two result sets are merged and re-ranked by a combined score:

```
score = α × vector_similarity + (1 - α) × graph_proximity
```

`α` defaults to 0.7 (vector-heavy) but is configurable per query.

## When to Use Hybrid Search

- **Vector search alone** is fast and works well for broad similarity retrieval.
- **Hybrid search** is better when you want to retrieve not just similar nodes but also their context — related decisions, causes, patterns, and goals.

Example: searching for "authentication" might return a `fact` node about JWT. Hybrid search would also surface the `decision` node that chose JWT and the `event` nodes where authentication errors occurred.

## API

### gRPC

```protobuf
rpc HybridSearch(HybridSearchRequest) returns (SearchResponse);

message HybridSearchRequest {
  string query = 1;
  uint32 limit = 2;
  float alpha = 3;          // 0.0 = pure graph, 1.0 = pure vector
  uint32 graph_hops = 4;    // depth of graph expansion
}
```

### CLI

```bash
cortex search "authentication" --hybrid --alpha 0.7 --hops 2
```

### Python SDK

```python
results = cx.search("authentication", limit=10, hybrid=True, alpha=0.7)
```

## Temporal Filtering

Hybrid search accepts an optional `valid_at` parameter. When set, candidates are filtered to only include nodes that were valid at the requested time, after the vector pass but before the merge.

This lets you ask questions like "what did we know about authentication in January?" and get only the facts that were true at that time.

### CLI

```bash
cortex search "authentication" --hybrid --valid-at 2025-01-15T00:00:00Z
```

### gRPC

```protobuf
message HybridSearchRequest {
  string query = 1;
  uint32 limit = 2;
  float alpha = 3;
  uint32 graph_hops = 4;
  google.protobuf.Timestamp valid_at = 5;  // optional temporal filter
}
```

### Python SDK

```python
from datetime import datetime
results = cx.search("authentication", hybrid=True, valid_at=datetime(2025, 1, 15))
```
