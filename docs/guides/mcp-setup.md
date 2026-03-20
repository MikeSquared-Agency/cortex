# MCP Setup Guide

Cortex includes a native MCP server that runs in library mode: it opens
the database directly with no network hop. This makes it faster than
MCP servers that proxy through HTTP.

## Prerequisites

Install Cortex:

```bash
curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh
```

Initialise a project (creates cortex.toml):

```bash
cortex init
```

## Option A: Direct binary (recommended)

The `cortex mcp` command starts an MCP server using stdio transport.
It opens the redb database directly in library mode.

### Claude Code

```bash
claude mcp add cortex -- cortex mcp
```

To use a specific data directory:

```bash
claude mcp add cortex -- cortex mcp --data-dir /path/to/data
```

To connect to a running Cortex server instead of opening the DB directly:

```bash
cortex serve &
claude mcp add cortex -- cortex mcp --server http://localhost:9090
```

### Cursor

Create or edit `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp"]
    }
  }
}
```

### Windsurf

Add to your Windsurf MCP configuration:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp"]
    }
  }
}
```

### VS Code (GitHub Copilot)

Create `.vscode/mcp.json`:

```json
{
  "servers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp"]
    }
  }
}
```

### OpenCode / Gemini CLI

Add to your MCP config file:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex",
      "args": ["mcp"]
    }
  }
}
```

## Option B: Node.js bridge (no Rust needed)

The MCP bridge is a standalone Node.js script that proxies MCP requests
to a running Cortex HTTP server. Use this when you can't install the
Rust binary or want to connect to a remote server.

```bash
# Start Cortex server
cortex serve

# In another terminal, or via MCP config:
node mcp-bridge/cortex-mcp-bridge.js
```

MCP config:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "node",
      "args": ["mcp-bridge/cortex-mcp-bridge.js"],
      "env": {
        "CORTEX_URL": "http://localhost:9091"
      }
    }
  }
}
```

For a remote server:

```json
{
  "env": {
    "CORTEX_URL": "https://cortex.example.com",
    "CORTEX_AUTH_TOKEN": "your-token-here"
  }
}
```

## Option C: Docker

```bash
docker run -d --name cortex -p 9090:9090 -p 9091:9091 \
  -v cortex-data:/data mikesquared/cortex:latest

# Then use the Node.js bridge to connect
CORTEX_URL=http://localhost:9091 node mcp-bridge/cortex-mcp-bridge.js
```

## Available MCP tools

### cortex_store

Store knowledge in the graph.

Parameters:
- `kind` (string): fact, decision, goal, event, pattern, observation, or any custom kind
- `title` (string, required): short summary
- `body` (string): full content
- `tags` (array): tags for categorisation
- `importance` (number): 0.0-1.0, affects ranking and decay resistance

### cortex_search

Semantic search by meaning. Returns nodes ranked by embedding similarity.

Parameters:
- `query` (string, required): what to search for
- `limit` (integer): max results (default 10)

### cortex_recall

Hybrid search combining semantic similarity with graph structure.
Better than cortex_search when you need contextually related information.

Parameters:
- `query` (string, required): what to recall
- `limit` (integer): max results
- `hops` (integer): graph traversal depth (default 2)

### cortex_briefing

Generate a structured context document for an agent. Use at session start
to load relevant context.

Parameters:
- `agent_id` (string, required): which agent's perspective
- `compact` (boolean): dense format for smaller context windows
- `scope` (string): "agent" (default), "shared" (cross-agent), or "unified"
- `agents` (array): agent IDs for unified scope

### cortex_traverse

Explore connections from a node in the graph.

Parameters:
- `node_id` (string, required): starting node
- `depth` (integer): how many hops (default 2)
- `relations` (array): filter by relation types

### cortex_relate

Create an explicit relationship between two nodes.

Parameters:
- `from` (string, required): source node ID
- `to` (string, required): target node ID
- `relation` (string): relationship type (default "relates_to")
- `weight` (number): relationship strength 0.0-1.0

### cortex_observe

Record a performance observation for prompt selection feedback.

Parameters:
- `agent_id` (string, required): observing agent
- `variant_id` (string): prompt variant UUID
- `sentiment_score` (number): -1.0 to 1.0
- `task_outcome` (string): success, failure, partial

## Best practices

1. **Start every session with a briefing**: `cortex_briefing(agent_id="your-agent", compact=true)`
2. **Store decisions with rationale**: include why, not just what
3. **Use importance scores**: 0.9 for architectural decisions, 0.3 for ephemeral observations
4. **Tag consistently**: tags enable cross-cutting queries
5. **Let the auto-linker work**: don't manually link everything; store facts and let Cortex discover connections

## Troubleshooting

### "command not found: cortex"

The binary isn't in your PATH. Either:
- Run the install script again: `curl -sSf ... | sh`
- Or specify the full path in your MCP config: `"command": "/usr/local/bin/cortex"`

### MCP server starts but tools don't appear

Check that the data directory exists and is writable:
```bash
cortex mcp --data-dir ./data 2>/dev/null
# Should output MCP JSON-RPC on stdout
```

### "Database not found"

Run `cortex init` first, or specify `--data-dir` pointing to an existing Cortex project.

### Tools are slow on first use

The embedding model downloads on first use (~80MB). Subsequent starts are instant.
