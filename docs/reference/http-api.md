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
