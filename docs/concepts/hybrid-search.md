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
