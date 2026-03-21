# Cortex

[![CI](https://github.com/MikeSquared-Agency/cortex/actions/workflows/ci.yml/badge.svg)](https://github.com/MikeSquared-Agency/cortex/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/MikeSquared-Agency/cortex)](https://github.com/MikeSquared-Agency/cortex/releases/latest)
[![crates.io](https://img.shields.io/crates/v/cortex-memory.svg)](https://crates.io/crates/cortex-memory)
[![PyPI](https://img.shields.io/pypi/v/cortex-memory.svg)](https://pypi.org/project/cortex-memory/)
[![npm](https://img.shields.io/npm/v/@cortex-memory/client.svg)](https://www.npmjs.com/package/@cortex-memory/client)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Self-organizing graph memory for AI agents. One binary. Zero dependencies.**

Cortex is an embedded temporal graph memory that auto-links knowledge, decays what's irrelevant, detects contradictions, and synthesises context briefings on demand. Think SQLite, but for agent memory.

## Why Cortex?

Your agents each know something. Cortex discovers what they know together.

Most agent memory systems are glorified key-value stores with a vector index. Cortex is a self-organizing knowledge graph: it wires itself, forgets what's stale, resolves contradictions, computes trust from topology, and tells each agent exactly what it needs to know.

**Self-organizing** -- the auto-linker runs in the background, discovering relationships via embedding similarity, shared entities, temporal proximity, and configurable structural rules. You store facts; Cortex finds the connections.

**Temporal** -- every node carries validity windows (`valid_from`, `valid_until`) and lifecycle expiry (`expires_at`). Query "facts true on January 15th" or "facts that expired before June." Stale knowledge is filtered, not deleted.

**Trust from topology** -- confidence is not a field you set. It's computed from the graph structure: corroboration across agents, contradiction count, source track record, access reinforcement, and freshness. Like PageRank for knowledge.

**Briefing synthesis** -- "what does my agent need to know?" generates a structured context document from the graph. Configurable roles, agent or cross-agent scope, contradiction alerts.

**Embedded** -- single Rust binary, redb storage (ACID, mmap), HNSW vector index. No external dependencies. No server to manage. `cargo install` and go.

## Quick Start

### Install

```bash
# Linux / macOS -- one line, no Rust or protoc needed
curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh

# Via Cargo
cargo install cortex-memory

# Docker
docker run -p 9090:9090 -p 9091:9091 mikesquared/cortex:latest
```

### 5 Minutes to Memory

```bash
# Create a project
cortex init

# Start the server
cortex serve

# Store some knowledge
cortex node create --kind fact --title "The API uses JWT auth" --importance 0.7

# Store with temporal validity
cortex node create --kind fact --title "Rate limit is 1000/min" \
  --valid-from 2026-01-01T00:00:00Z

# Search
cortex search "authentication"

# Get a briefing for your agent
cortex briefing my-agent

# Get a cross-agent briefing
cortex briefing my-agent --scope shared

# Check trust score for a node
cortex trust <node-id>
```

## Use with AI Agents (MCP)

Cortex speaks MCP natively. One command gives your AI agent persistent,
self-organizing memory.

### Claude Code

```bash
# Start Cortex in the background
cortex serve &

# Add to Claude Code
claude mcp add cortex -- cortex mcp
```

Claude Code now has 7 memory tools: `cortex_store`, `cortex_search`,
`cortex_recall`, `cortex_briefing`, `cortex_traverse`, `cortex_relate`,
`cortex_observe`.

### Cursor

Add to `.cursor/mcp.json`:

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

Add to your MCP config:

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

### VS Code (Copilot)

Add to `.vscode/mcp.json`:

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

### Without installing the binary

Use the Node.js bridge (no Rust needed):

```bash
npx cortex-mcp-bridge
```

Or with a remote Cortex server:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "node",
      "args": ["/path/to/cortex-mcp-bridge.js"],
      "env": {
        "CORTEX_URL": "http://your-server:9091"
      }
    }
  }
}
```

### What your agent gets

| Tool | Purpose |
|------|---------|
| `cortex_store` | Remember facts, decisions, goals, events, patterns |
| `cortex_search` | Semantic search by meaning |
| `cortex_recall` | Hybrid search (semantic + graph structure) |
| `cortex_briefing` | "What do I need to know?" context document |
| `cortex_traverse` | Explore how concepts connect in the graph |
| `cortex_relate` | Explicitly link related knowledge |
| `cortex_observe` | Record performance metrics for prompt selection |

The briefing tool supports scope: `agent` (default), `shared` (cross-agent),
or `unified` (multi-agent overview for orchestrators).

### Prompt Management

Cortex includes a built-in prompt versioning system with branching, inheritance, and context-aware selection.

```bash
# Import prompts from a migration file
cortex prompt migrate prompts.json

# List all prompts
cortex prompt list

# Get the resolved HEAD of a prompt (with inherited sections)
cortex prompt get my-prompt

# Bind a prompt to an agent
cortex agent bind my-agent my-prompt --weight 0.9

# Select the best variant for current context (epsilon-greedy)
cortex agent select my-agent --task-type coding --sentiment 0.3

# Record performance and auto-update weights
cortex agent observe my-agent \
  --variant-id UUID --variant-slug my-prompt \
  --sentiment-score 0.8 --task-outcome success

# Check performance metrics
cortex prompt performance my-prompt
```

### As a Library (Python)

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
cx.store("decision", "Use FastAPI", body="Async + type hints", importance=0.8)

results = cx.search("backend framework")
print(cx.briefing("my-agent"))

# Cross-agent briefing
print(cx.briefing("my-agent", scope="shared"))

# Store an entity
cx.store_entity("Anthropic", entity_type="company", aliases=["anthropic-ai"])
```

### Embedded in Rust

```rust
use cortex_core::{Cortex, LibraryConfig};

let cx = Cortex::open("./memory.redb", LibraryConfig::default())?;
cx.store(Node::fact("The API uses JWT auth", 0.7))?;
let results = cx.search("authentication", 5)?;
```

## Documentation

- **[Quick Start](docs/getting-started/quickstart.md)**
- **[MCP Setup](docs/guides/mcp-setup.md)** -- Claude Code, Cursor, Windsurf, VS Code
- **[Configuration Reference](docs/reference/config.md)**
- **[CLI Reference](docs/reference/cli.md)**
- **[Python SDK](docs/reference/python-sdk.md)**
- **[gRPC API](docs/reference/grpc-api.md)**
- **[HTTP API](docs/reference/http-api.md)**
- **[Prompt System](docs/concepts/prompt-system.md)**
- **[Architecture](docs/concepts/architecture.md)**

### Concepts

- **[Self-Organizing Memory](docs/concepts/self-organizing-memory.md)** -- how Cortex discovers what your agents know together
- **[Graph Model](docs/concepts/graph-model.md)** -- nodes, edges, temporal validity, entities
- **[Trust Scoring](docs/concepts/trust-scoring.md)** -- PageRank for knowledge
- **[Entity Resolution](docs/concepts/entity-resolution.md)** -- cross-agent discovery via shared entities
- **[Briefings](docs/concepts/briefings.md)** -- role-based context synthesis with scope
- **[Auto-Linker](docs/concepts/auto-linker.md)** -- configurable rules, entity promotion
- **[Decay and Memory](docs/concepts/decay-and-memory.md)** -- edge decay vs temporal validity
- **[Metadata Conventions](docs/reference/metadata-conventions.md)** -- well-known metadata keys

### Guides

- **[Coding Agent](docs/guides/coding-agent.md)** -- memory for coding agents
- **[Research Agent](docs/guides/research-agent.md)** -- memory for research agents
- **[Browser Agent](docs/guides/browser-agent.md)** -- memory for browser agents
- **[Multi-Agent](docs/guides/multi-agent.md)** -- shared memory, entities, scoped briefings

## How Cortex Compares

| | Cortex | Mem0 | Zep/Graphiti | Cognee | Letta | Engram |
|---|---|---|---|---|---|---|
| Embedded (no server) | Yes | No | No | No | No | Yes |
| Self-organizing graph | Yes | No | Partial | Partial | No | No |
| Auto-linking | Yes | No | No | No | No | No |
| Temporal validity | Yes | No | Yes | No | No | No |
| Knowledge decay | Yes | No | No | Partial | No | Yes |
| Contradiction detection | Yes | No | Yes | No | No | No |
| Trust from topology | Yes | No | No | No | No | No |
| Briefing synthesis | Yes | No | No | No | No | No |
| Entity resolution | Yes | Yes | Yes | Yes | No | No |
| Cross-agent discovery | Yes | Yes | No | Yes | No | No |
| Single binary | Yes | No | No | No | No | Yes |
| Rust (performance) | Yes | No | No | No | No | No (Go) |
| Open source (MIT) | Yes | Partial | Partial | Yes | Yes | Yes |

Cortex occupies a unique position: embedded AND self-organizing. Every competitor requires external infrastructure or manual curation. Cortex does neither.

## Graph Visualisation

Cortex ships a live graph explorer. Start the server and open [http://localhost:9091/viz](http://localhost:9091/viz):

- Force-directed layout with nodes coloured by kind
- Node size reflects importance score
- Click any node for full details (title, body, metadata, connections)
- Search, filter by kind, filter by minimum importance
- Export as SVG, PNG, or JSON

## Architecture

```
┌────────────────────────────────────────────────┐
│              Your Application                  │
│       AI Agent(s)    SDK / gRPC / MCP          │
└──────────────────┬─────────────────────────────┘
                   │
┌──────────────────▼─────────────────────────────┐
│                   Cortex                       │
│  gRPC :9090           HTTP :9091               │
│  ┌───────────┐  ┌────────────┐  ┌──────────┐  │
│  │  Storage   │  │   Graph    │  │   HNSW   │  │
│  │  (redb)    │  │   Engine   │  │   Index  │  │
│  └───────────┘  └────────────┘  └──────────┘  │
│  ┌───────────┐  ┌────────────┐  ┌──────────┐  │
│  │ Auto-Link  │  │  Briefing  │  │  Trust   │  │
│  │ + Entities │  │  + Scope   │  │  Engine  │  │
│  └───────────┘  └────────────┘  └──────────┘  │
│  ┌───────────┐  ┌────────────┐  ┌──────────┐  │
│  │  Prompt    │  │ Selection  │  │ Retention│  │
│  │  Resolver  │  │  Engine    │  │ + Expiry │  │
│  └───────────┘  └────────────┘  └──────────┘  │
└────────────────────────────────────────────────┘
```

## Integration Guides

- **[LangChain](docs/guides/langchain.md)** -- Use Cortex as a LangChain memory backend
- **[CrewAI](docs/guides/crewai.md)** -- Share memory across a multi-agent team
- **[OpenClaw / Warren](docs/guides/openclaw.md)** -- Native integration with Warren

## Examples

```
examples/
  langchain-agent/     LangChain agent with Cortex memory
  crewai-team/         CrewAI multi-agent with shared Cortex
  personal-assistant/  Simple assistant with briefings
  rag-pipeline/        Cortex as a RAG backend
  rust-embedded/       Rust app using cortex-core directly
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All contributions welcome.

## License

MIT -- see [LICENSE](LICENSE).
