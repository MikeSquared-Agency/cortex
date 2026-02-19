> **Status:** READY TO IMPLEMENT

# Spec 08 — MCP Server

## Overview

A Model Context Protocol (MCP) server that exposes Cortex graph memory as tools and resources to any MCP-compatible AI agent. Ships as a subcommand: `cortex mcp`.

**Dependency:** Phases 7a + 7b (library mode). No server needed — uses `Cortex::open()` directly.

## Architecture

```
┌──────────────┐     stdio (JSON-RPC)     ┌──────────────┐
│  MCP Client  │ ◄──────────────────────► │  cortex mcp  │
│  (Claude,    │                          │              │
│   Cursor,    │                          │  Cortex::    │
│   Cline)     │                          │  open(path)  │
└──────────────┘                          └──────┬───────┘
                                                 │
                                          ┌──────▼───────┐
                                          │    redb      │
                                          │  (on disk)   │
                                          └──────────────┘
```

**Key design:** Embedded library mode, NOT a gRPC client. The MCP server opens the database directly via `Cortex::open()`. This means:
- Zero network dependencies
- Works offline
- Single process, single binary
- No need to run `cortex serve` separately

If the user wants to connect to a remote server instead, a `--server` flag falls back to the gRPC client.

## MCP Client Config

```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp", "--data-dir", "~/.cortex/default"]
    }
  }
}
```

Or connecting to a running server:
```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp", "--server", "http://localhost:9090"]
    }
  }
}
```

## Tools

### `cortex_store`

Store a knowledge node in the graph.

```json
{
  "name": "cortex_store",
  "description": "Store a piece of knowledge in persistent graph memory. Use this to remember facts, decisions, goals, events, patterns, and observations across sessions.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "kind": {
        "type": "string",
        "description": "Node type: fact, decision, goal, event, pattern, observation, preference",
        "default": "fact"
      },
      "title": {
        "type": "string",
        "description": "Short summary (used for search and dedup)"
      },
      "body": {
        "type": "string",
        "description": "Full content. Can be long."
      },
      "tags": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Optional tags for filtering"
      },
      "importance": {
        "type": "number",
        "description": "0.0 to 1.0. Higher = retained longer, weighted more in search.",
        "default": 0.5
      }
    },
    "required": ["title"]
  }
}
```

**Returns:** `{ "id": "<uuid>", "message": "Stored: <title>" }`

### `cortex_search`

Semantic similarity search across the graph.

```json
{
  "name": "cortex_search",
  "description": "Search graph memory by meaning. Returns the most relevant nodes ranked by semantic similarity.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Natural language search query"
      },
      "limit": {
        "type": "integer",
        "description": "Max results to return",
        "default": 10
      },
      "kind": {
        "type": "string",
        "description": "Optional: filter by node kind"
      }
    },
    "required": ["query"]
  }
}
```

**Returns:** Array of `{ "id", "kind", "title", "body", "score", "created_at" }`

### `cortex_recall`

Hybrid search combining vector similarity and graph proximity. More contextual than pure search.

```json
{
  "name": "cortex_recall",
  "description": "Recall knowledge using hybrid search (semantic + graph structure). Better than search when you need contextually related information, not just similar text.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "What to recall"
      },
      "limit": {
        "type": "integer",
        "default": 10
      },
      "alpha": {
        "type": "number",
        "description": "Balance: 0.0 = pure graph, 1.0 = pure vector. Default 0.7",
        "default": 0.7
      }
    },
    "required": ["query"]
  }
}
```

**Returns:** Same format as search, with hybrid scores.

### `cortex_briefing`

Generate a context briefing — a synthesised summary of relevant knowledge.

```json
{
  "name": "cortex_briefing",
  "description": "Generate a context briefing from graph memory. Returns a structured summary of relevant knowledge including active goals, recent decisions, patterns, and key facts. Use at the start of a session or when you need a broad overview.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "agent_id": {
        "type": "string",
        "description": "Agent identifier for personalised briefings",
        "default": "default"
      },
      "compact": {
        "type": "boolean",
        "description": "If true, returns a shorter ~4x denser briefing",
        "default": false
      }
    }
  }
}
```

**Returns:** `{ "briefing": "<rendered markdown>" }`

### `cortex_traverse`

Explore the graph from a starting node.

```json
{
  "name": "cortex_traverse",
  "description": "Explore connections from a node in the knowledge graph. Reveals how concepts relate to each other.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "node_id": {
        "type": "string",
        "description": "Starting node UUID"
      },
      "depth": {
        "type": "integer",
        "description": "How many hops to explore",
        "default": 2
      },
      "direction": {
        "type": "string",
        "enum": ["outgoing", "incoming", "both"],
        "default": "both"
      }
    },
    "required": ["node_id"]
  }
}
```

**Returns:** `{ "nodes": [...], "edges": [...] }` — the subgraph.

### `cortex_relate`

Create a typed relationship between two nodes.

```json
{
  "name": "cortex_relate",
  "description": "Create a relationship between two nodes in the knowledge graph. Use to explicitly connect related concepts.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "from_id": {
        "type": "string",
        "description": "Source node UUID"
      },
      "to_id": {
        "type": "string",
        "description": "Target node UUID"
      },
      "relation": {
        "type": "string",
        "description": "Relationship type: relates-to, supports, contradicts, caused-by, depends-on, similar-to, supersedes",
        "default": "relates-to"
      }
    },
    "required": ["from_id", "to_id"]
  }
}
```

**Returns:** `{ "id": "<edge-uuid>", "message": "Related: <from> → <relation> → <to>" }`

## Resources

### `cortex://stats`

```json
{
  "uri": "cortex://stats",
  "name": "Graph Statistics",
  "description": "Current graph memory statistics",
  "mimeType": "application/json"
}
```

Returns node count, edge count, per-kind breakdown, DB size, oldest/newest node.

### `cortex://node/{id}`

```json
{
  "uri": "cortex://node/{id}",
  "name": "Knowledge Node",
  "description": "A single node from graph memory",
  "mimeType": "application/json"
}
```

Returns the full node with metadata, edges, and related nodes.

## Implementation

### File: `crates/cortex-server/src/mcp/mod.rs`

```rust
use rmcp::{ServerHandler, tool, McpServer};

pub struct CortexMcp {
    cortex: Arc<CortexInstance>,  // Library mode handle
}

#[tool(description = "Store a piece of knowledge in persistent graph memory")]
async fn cortex_store(&self, kind: Option<String>, title: String, body: Option<String>, ...) -> Result<String> { ... }

#[tool(description = "Search graph memory by meaning")]
async fn cortex_search(&self, query: String, limit: Option<i32>, ...) -> Result<String> { ... }

// ... etc
```

### Crate: `rmcp`

Use the official Rust MCP SDK: https://github.com/anthropics/rust-mcp-sdk

```toml
[dependencies]
rmcp = { version = "0.1", features = ["server", "transport-io"] }
```

### CLI integration

Add `Mcp` variant to the CLI commands enum:

```rust
/// Start MCP server (stdio transport)
Mcp {
    /// Path to cortex data directory
    #[arg(long, default_value = "~/.cortex/default")]
    data_dir: Option<PathBuf>,

    /// Connect to remote server instead of opening locally
    #[arg(long)]
    server: Option<String>,
}
```

### Transport

stdio (stdin/stdout JSON-RPC) — this is what MCP clients expect. The MCP server reads JSON-RPC requests from stdin and writes responses to stdout. All logging goes to stderr.

## Definition of Done

- [ ] `cortex mcp` starts an MCP server on stdio
- [ ] All 6 tools callable from Claude Desktop / Cursor / Cline
- [ ] `cortex_store` creates nodes with embeddings
- [ ] `cortex_search` returns ranked results
- [ ] `cortex_recall` returns hybrid-scored results
- [ ] `cortex_briefing` returns rendered markdown briefing
- [ ] `cortex_traverse` returns subgraph as JSON
- [ ] `cortex_relate` creates typed edges
- [ ] Both resources (`stats`, `node/{id}`) return valid JSON
- [ ] Works in library mode (no server, no network)
- [ ] `--server` flag falls back to gRPC client for remote mode
- [ ] Integration test: mock MCP client → all 6 tools → verify responses
- [ ] README updated with MCP setup instructions
- [ ] Logs go to stderr, never stdout (stdout is the MCP transport)
