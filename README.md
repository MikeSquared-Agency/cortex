# Cortex

**Warren's Graph Memory Engine**

Embedded Rust knowledge graph with vector similarity, auto-linking, and agent briefings. The brain of the Warren swarm.

## What Is This?

Cortex is a local, embedded graph database purpose-built for AI agent memory. No external database dependencies. No JVM. No cloud. A single Rust binary that stores typed knowledge as graph nodes, automatically discovers relationships via embedding similarity, and synthesises context briefings for agents at boot.

## Why Build This?

Existing solutions fall into two camps:

1. **External databases** (Neo4j, SurrealDB, Postgres+pgvector) — network hop, operational overhead, another process to manage, another thing that goes down at 3am.
2. **Flat file memory** (MEMORY.md, AGENTS.md) — no relationships, no semantic search, stale within hours, doesn't scale past one agent.

Cortex is neither. It's an embedded library that runs in-process, stores everything on local disk, and grows its own knowledge graph automatically. Agents don't manage their memory — Cortex does it for them.

## Architecture

```
┌─────────────────────────────────────────────┐
│                  cortex-server               │
│  gRPC API │ HTTP debug │ NATS consumer       │
├─────────────────────────────────────────────┤
│                  cortex-core                 │
│  ┌──────────┐ ┌───────────┐ ┌─────────────┐ │
│  │  Storage  │ │  Graph    │ │  Auto-Link  │ │
│  │  (redb)   │ │  Engine   │ │  (Cortex)   │ │
│  └──────────┘ └───────────┘ └─────────────┘ │
│  ┌──────────┐ ┌───────────┐ ┌─────────────┐ │
│  │  Vector   │ │  Briefing │ │  Ingest     │ │
│  │  (HNSW)   │ │  Synth    │ │  Pipeline   │ │
│  └──────────┘ └───────────┘ └─────────────┘ │
├─────────────────────────────────────────────┤
│              cortex-proto                    │
│  gRPC service definitions (protobuf)        │
└─────────────────────────────────────────────┘
```

## Phases

| Phase | Name | Spec | Duration |
|-------|------|------|----------|
| 1 | Foundation — Storage & Data Model | [specs/01-foundation.md](specs/01-foundation.md) | 1 week |
| 2 | Graph Engine — Traversal & Query | [specs/02-graph-engine.md](specs/02-graph-engine.md) | 1 week |
| 3 | Vector Layer — Embeddings & Similarity | [specs/03-vector-layer.md](specs/03-vector-layer.md) | 1 week |
| 4 | Auto-Linker — Self-Growing Graph | [specs/04-auto-linker.md](specs/04-auto-linker.md) | 1 week |
| 5 | API & Integration — Wire Into Warren | [specs/05-api-integration.md](specs/05-api-integration.md) | 1 week |
| 6 | Cortex Briefings — Agent Context Synthesis | [specs/06-briefings.md](specs/06-briefings.md) | 1 week |

## Tech Stack

| Component | Choice | Why |
|-----------|--------|-----|
| Language | Rust | CPU-bound graph traversal + vector math. Ownership model for concurrent access. Single binary. |
| Graph Storage | redb | Pure Rust embedded KV. Zero-copy mmap reads. Used by Spacebot. |
| Vector Index | HNSW (instant-distance or hora) | In-process ANN search. No external service. |
| Embeddings | FastEmbed-rs | Local embedding generation. No API calls. No network dependency. |
| gRPC | tonic | Rust-native, high-performance. Schema-enforced API. |
| Serialization | bincode + serde | Compact binary format for storage. JSON for API/debug. |
| Async Runtime | tokio | Standard. Shared with tonic and NATS client. |

## Repo Structure

```
cortex/
├── Cargo.toml              # Workspace root
├── specs/                   # Phase specifications
│   ├── 01-foundation.md
│   ├── 02-graph-engine.md
│   ├── 03-vector-layer.md
│   ├── 04-auto-linker.md
│   ├── 05-api-integration.md
│   └── 06-briefings.md
├── proto/                   # Protobuf definitions
│   └── cortex.proto
├── crates/
│   ├── cortex-core/         # Library: storage, graph, vector, auto-link
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── cortex-server/       # Binary: gRPC + HTTP + NATS
│   │   ├── Cargo.toml
│   │   └── src/
│   └── cortex-proto/        # Generated gRPC code
│       ├── Cargo.toml
│       └── src/
├── Dockerfile
└── README.md
```

## License

MIT
