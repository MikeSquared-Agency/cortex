# Cortex

**Embedded graph memory for AI agents. One binary. One file. Zero dependencies.**

Cortex is a local knowledge graph that stores what your AI agents know, automatically discovers relationships between knowledge, and synthesises context briefings on demand. Think SQLite, but for agent memory.

## Why Cortex?

Your agent's memory shouldn't be a text file. It should be a living graph that wires itself, forgets what's irrelevant, and tells your agent exactly what it needs to know.

- **Graph-native** — typed nodes and edges, not just vectors
- **Auto-linking** — relationships discovered via embedding similarity
- **Decay** — unused knowledge fades, important knowledge persists
- **Briefings** — "what do I need to know?" → tailored context document
- **Hybrid search** — vector similarity × graph proximity
- **Embedded** — single file, no external dependencies
- **Fast** — Rust, HNSW index, mmap'd storage

## Quick Start

### Install

```bash
# Cargo
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

# Search
cortex search "authentication"

# Get a briefing for your agent
cortex briefing my-agent
```

### As a Library (Python)

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
cx.store("decision", "Use FastAPI", body="Async + type hints", importance=0.8)

results = cx.search("backend framework")
print(cx.briefing("my-agent"))
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
- **[Configuration Reference](docs/reference/config.md)**
- **[CLI Reference](docs/reference/cli.md)**
- **[Python SDK](docs/reference/python-sdk.md)**
- **[gRPC API](docs/reference/grpc-api.md)**
- **[Architecture](docs/concepts/architecture.md)**

## Why Not a Vector DB?

| Feature | Cortex | Mem0 | Zep | Chroma | pgvector |
|---------|--------|------|-----|--------|----------|
| Embedded (no server) | ✅ | ❌ | ❌ | ✅ | ❌ |
| Graph relationships | ✅ native | ❌ | ❌ | ❌ | ❌ |
| Auto-linking | ✅ | ❌ | ❌ | ❌ | ❌ |
| Edge decay | ✅ | ❌ | ❌ | ❌ | ❌ |
| Contradiction detection | ✅ | ❌ | ❌ | ❌ | ❌ |
| Briefing synthesis | ✅ | ❌ | ❌ | ❌ | ❌ |
| Hybrid search (vector+graph) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Local embeddings | ✅ | ❌ | ❌ | ✅ | ❌ |
| Single binary | ✅ | ❌ | ❌ | ❌ | ❌ |

**Our moat:** Graph-native memory with auto-linking and decay. Nobody else does this.

## Graph Visualisation

Cortex ships a live graph explorer. Start the server and open [http://localhost:9091/viz](http://localhost:9091/viz):

- Force-directed layout with nodes coloured by kind
- Node size reflects importance score
- Click any node for full details (title, body, metadata, connections)
- Search, filter by kind, filter by minimum importance
- Export as SVG, PNG, or JSON

## Architecture

```
┌────────────────────────────────────────────┐
│              Your Application              │
│         AI Agent   SDK / gRPC client       │
└─────────────────┬──────────────────────────┘
                  │
┌─────────────────▼──────────────────────────┐
│                  Cortex                    │
│  gRPC :9090          HTTP :9091            │
│  ┌──────────┐  ┌───────────┐  ┌─────────┐  │
│  │  Storage  │  │  Graph    │  │  HNSW   │  │
│  │  (redb)   │  │  Engine   │  │  Index  │  │
│  └──────────┘  └───────────┘  └─────────┘  │
│  ┌──────────┐  ┌───────────┐  ┌─────────┐  │
│  │Auto-Link │  │ Briefing  │  │  Ingest │  │
│  │(background)│ │  Engine   │  │ Pipeline│  │
│  └──────────┘  └───────────┘  └─────────┘  │
└────────────────────────────────────────────┘
```

## Integration Guides

- **[LangChain](docs/guides/langchain.md)** — Use Cortex as a LangChain memory backend
- **[CrewAI](docs/guides/crewai.md)** — Share memory across a multi-agent team
- **[OpenClaw / Warren](docs/guides/openclaw.md)** — Native integration with Warren

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

MIT — see [LICENSE](LICENSE).
