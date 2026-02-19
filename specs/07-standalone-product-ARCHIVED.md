# Cortex — Standalone Product Spec

**Goal:** Transform Cortex from a Warren-internal component into a standalone, open-source graph memory engine that any AI agent framework can use.

**Tagline:** *Embedded graph memory for AI agents. One binary. One file. Zero dependencies.*

---

## The Pitch

Every AI agent framework has the same memory problem: flat files, vector databases bolted on as afterthoughts, or external graph databases that add operational complexity. Cortex is the first embedded graph memory engine purpose-built for agents — local storage, automatic relationship discovery, semantic search, and context briefings in a single Rust binary.

**Think SQLite for agent memory.** No server to manage. No cloud dependency. Just a library you embed or a binary you run alongside your agent.

---

## Target Users

1. **Agent framework developers** — building on LangChain, CrewAI, AutoGen, OpenClaw, custom frameworks. They need persistent, structured memory that goes beyond "append to a text file."

2. **Solo developers / indie hackers** — building personal AI assistants. Don't want to run Postgres + pgvector + a separate graph DB. Want one thing that does it all.

3. **Enterprise teams** — need local/on-prem memory (data sovereignty). Can't send everything to cloud vector DBs. Need audit trails (who knew what, when).

---

## What Changes

### 1. Configurable Node Kinds (replaces hardcoded enum)

**Current:** 8 fixed kinds (Agent, Decision, Fact, Event, Goal, Preference, Pattern, Observation).

**Target:** User-defined kinds via config, with the 8 defaults as a starter template.

```rust
// cortex.toml
[schema]
node_kinds = [
    "agent",
    "decision", 
    "fact",
    "event",
    "goal",
    "preference",
    "pattern",
    "observation",
    # User can add their own:
    "conversation",
    "document",
    "entity",
    "action",
]
```

Implementation:
- `NodeKind` becomes a newtype wrapper around `String` instead of an enum
- Default kinds provided by `NodeKindRegistry::default()`
- Validation: kinds must be lowercase alphanumeric + hyphens
- Storage layer: no change (already stores kind as u8/string)
- Auto-linker structural rules become configurable per-kind-pair

```rust
pub struct NodeKind(String);

impl NodeKind {
    pub fn new(kind: &str) -> Result<Self> { ... }
    pub fn as_str(&self) -> &str { &self.0 }
}

// Built-in convenience constants
pub const AGENT: NodeKind = NodeKind("agent");
pub const DECISION: NodeKind = NodeKind("decision");
// ...
```

### 2. Configurable Relations

Same pattern as kinds:

```rust
// cortex.toml
[schema]
relations = [
    "informed_by",
    "led_to",
    "applies_to",
    "contradicts",
    "supersedes",
    "depends_on",
    "related_to",
    "instance_of",
    # Custom:
    "mentions",
    "authored_by",
    "part_of",
]
```

### 3. Adapter Pattern for Event Ingest

**Current:** NATS consumer hardcoded to `warren.*` subjects.

**Target:** Pluggable ingest adapters.

```rust
pub trait IngestAdapter: Send + Sync + 'static {
    /// Subscribe to events and return a stream of IngestEvents.
    async fn subscribe(&self) -> Result<BoxStream<IngestEvent>>;
}

pub struct IngestEvent {
    pub kind: String,           // Maps to NodeKind
    pub title: String,
    pub body: String,
    pub metadata: HashMap<String, Value>,
    pub tags: Vec<String>,
    pub source: String,         // Which adapter produced this
    pub session: Option<String>,
}
```

Built-in adapters:
- **NATS** — subscribe to configurable subjects (default: `cortex.>`)
- **Webhook** — HTTP POST endpoint that accepts IngestEvents
- **File watcher** — existing file ingest, upgraded with inotify/fsevents
- **Stdin** — pipe JSON lines for scripting/testing

Warren-specific NATS mapping moves to a `warren-adapter` crate (separate, optional).

```toml
# cortex.toml
[ingest.nats]
url = "nats://localhost:4222"
subjects = ["myapp.>"]

[ingest.webhook]
enabled = true
port = 9092
auth_token = "secret"

[ingest.file]
watch_dir = "/data/ingest"
```

### 4. Configurable Briefing Sections

**Current:** 6 hardcoded sections (Identity, Active Context, Patterns, Goals, Unresolved, Recent Events).

**Target:** User-defined section templates.

```toml
# cortex.toml
[[briefing.sections]]
name = "Identity"
query = "kind:agent AND source:{agent_id}"
traversal = { direction = "both", relation = "applies_to", depth = 1 }
max_items = 5

[[briefing.sections]]
name = "Recent Activity"
query = "source:{agent_id} AND created_after:48h"
sort = "created_at:desc"
max_items = 10

[[briefing.sections]]
name = "Related Knowledge"
mode = "hybrid_search"
anchors = "recent"
vector_weight = 0.7
max_items = 10
```

Each section has a `mode`:
- `filter` — simple storage query
- `traversal` — graph traversal from anchor nodes
- `hybrid_search` — vector + graph proximity
- `contradictions` — special mode that finds Contradicts edges

### 5. Configuration File

Single `cortex.toml` for everything:

```toml
[server]
grpc_addr = "0.0.0.0:9090"
http_addr = "0.0.0.0:9091"
data_dir = "./data"

[schema]
node_kinds = ["agent", "decision", "fact", "event", "goal", "preference", "pattern", "observation"]
relations = ["informed_by", "led_to", "applies_to", "contradicts", "supersedes", "depends_on", "related_to", "instance_of"]

[embedding]
model = "BAAI/bge-small-en-v1.5"  # Downloaded automatically on first run
# Or: model = "custom" + dimension = 768 (bring your own via API)

[auto_linker]
enabled = true
interval_seconds = 60
similarity_threshold = 0.75
dedup_threshold = 0.92
decay_rate_per_day = 0.01
max_edges_per_node = 50

[briefing]
cache_ttl_seconds = 300
max_total_items = 50
max_chars = 8000
precompute_agents = ["default"]

[[briefing.sections]]
name = "Identity"
# ...

[ingest.file]
watch_dir = "./data/ingest"

[ingest.webhook]
enabled = false
```

### 6. SDK / Client Libraries

For agents to talk to Cortex without writing raw gRPC:

```python
# Python SDK (pip install cortex-memory)
from cortex_memory import CortexClient

client = CortexClient("localhost:9090")

# Store knowledge
client.create_node(
    kind="decision",
    title="Use FastAPI for the backend",
    body="Chose FastAPI over Flask for async support and type hints",
    tags=["backend", "python"],
    importance=0.8,
)

# Search
results = client.search("backend technology choices", limit=5)

# Get briefing
briefing = client.briefing("my-agent")
print(briefing.text)  # Ready-to-inject markdown
```

Also: TypeScript/Node, Go, Rust (native).

### 7. Library Mode (no server)

For embedding directly into an application:

```rust
// Rust — use as a library, no server needed
use cortex_core::{Cortex, Config};

let cortex = Cortex::open("./my-memory.redb", Config::default())?;

cortex.store(Node::fact("The API uses JWT auth", 0.7))?;

let results = cortex.search("authentication", 5)?;
let briefing = cortex.briefing("my-agent")?;
```

This is the SQLite model — most users don't run sqlite3 as a server, they embed it. Same here.

---

## Distribution

### Packaging
- **crates.io** — `cortex-core` (library) + `cortex-server` (binary)
- **Docker Hub** — `mikesquared/cortex:latest`
- **Homebrew** — `brew install cortex-memory`
- **pip** — `pip install cortex-memory` (Python SDK)
- **npm** — `npm install @cortex/client` (TypeScript SDK)

### Documentation
- **docs.cortex.dev** (or subdomain of darlington.dev)
- Quick start: "Memory for your AI agent in 5 minutes"
- Guides: embedding in Python agents, LangChain integration, CrewAI integration
- API reference: gRPC + HTTP + library
- Architecture: how the auto-linker works, how briefings are generated

### Examples Repository
- `examples/langchain-agent` — LangChain agent with Cortex memory
- `examples/crewai-team` — CrewAI multi-agent with shared Cortex
- `examples/personal-assistant` — Simple assistant with briefings
- `examples/rag-pipeline` — Cortex as a RAG backend (hybrid retrieval)

---

## Competitive Positioning

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
| Typed knowledge | ✅ | ❌ | ✅ | ❌ | ❌ |
| Open source | ✅ | ✅ | ✅ | ✅ | ✅ |
| Single binary | ✅ | ❌ | ❌ | ❌ | ❌ |
| Rust performance | ✅ | ❌ | ❌ | ✅* | ❌ |

*Chroma has a Rust core but Python server layer.

**Our moat:** Graph-native memory with auto-linking and decay. Nobody else does this. Vector DBs give you similarity search. We give you a knowledge graph that grows itself.

---

## Naming

Options:
- **Cortex** — current name. Strong, memorable. Risk: common word, may have trademark conflicts.
- **cortex-db** — more specific, less conflict risk.
- **graphmem** — descriptive, available on crates.io.
- **engram** — neuroscience term for a memory trace. Unique, memorable.

Recommend: **Cortex** if we can secure the namespace, **Engram** as backup.

---

## Revenue Model (Optional — can stay open source)

1. **Cortex Cloud** — managed hosted version with team features, access control, dashboard. Free tier (1 agent, 10k nodes) → paid (unlimited, $29/mo per agent).

2. **Enterprise license** — self-hosted, support, SLA, custom integrations. Annual contract.

3. **Marketplace** — pre-built adapter packs (Slack ingest, GitHub ingest, email ingest). Free core + paid adapters.

---

## Build Order

### Week 1-2: Core Decoupling
- [ ] NodeKind → string newtype with registry
- [ ] Relation → string newtype with registry
- [ ] Configuration file parser (cortex.toml)
- [ ] Extract Warren NATS mapping to `warren-adapter` crate
- [ ] Adapter trait for ingest

### Week 3: Briefing Configurability
- [ ] Section template DSL
- [ ] Move hardcoded sections to default config
- [ ] Configurable briefing via cortex.toml

### Week 4: Library Mode
- [ ] `Cortex::open()` convenience API
- [ ] No-server usage path
- [ ] Examples: basic_usage, rag_pipeline

### Week 5: Distribution
- [ ] Publish cortex-core and cortex-server to crates.io
- [ ] Docker Hub image with CI
- [ ] Homebrew formula
- [ ] README rewrite for public audience

### Week 6: SDKs
- [ ] Python SDK (grpcio-tools generated + convenience layer)
- [ ] TypeScript SDK
- [ ] Integration examples (LangChain, CrewAI)

### Week 7-8: Documentation & Launch
- [ ] Documentation site
- [ ] Blog post: "Why we built our own graph memory engine"
- [ ] HN/Reddit launch
- [ ] X/LinkedIn announcement

---

## Success Metrics (first 90 days)

- GitHub stars: 500+
- crates.io downloads: 1000+
- pip installs: 500+
- External contributors: 5+
- Production users (self-reported): 10+
- One integration with a major agent framework (LangChain or CrewAI)

---

## CLI Tools

The `cortex` binary doubles as both the server and a comprehensive CLI toolkit.

### Commands

```
cortex serve                    Start the gRPC + HTTP server
cortex init                     Interactive setup wizard (generates cortex.toml)
cortex shell                    Interactive REPL for queries and exploration
cortex stats                    Graph health overview (nodes, edges, kinds, size)
cortex doctor                   Diagnose issues (corrupt index, orphans, stale embeddings)

cortex node create              Create a node interactively or from flags
cortex node get <id>            Get a node by ID
cortex node list [--kind X]     List/filter nodes
cortex node delete <id>         Soft-delete a node

cortex edge create              Create an edge
cortex edge list [--from X]     List edges

cortex search <query>           Semantic similarity search
cortex search --hybrid <query>  Hybrid search (vector + graph)
cortex traverse <id> [--depth]  Graph traversal from a node
cortex path <from> <to>         Find shortest path

cortex briefing <agent_id>      Generate and print a briefing
cortex briefing --compact       Compact mode

cortex import <file>            Bulk import (JSON, CSV, markdown, JSONL)
cortex export [--format json]   Export graph (JSON, DOT, GraphML, JSONL)
cortex backup <path>            Verified backup with optional encryption
cortex restore <path>           Restore from backup
cortex migrate                  Run schema migrations after upgrade

cortex config validate          Validate cortex.toml
cortex config show              Show resolved config (with defaults)
```

### Setup Wizard (`cortex init`)

Interactive walkthrough that generates a `cortex.toml`:

```
$ cortex init

Welcome to Cortex — graph memory for AI agents.

? Where should Cortex store data? [./data]
? Which embedding model? (use arrow keys)
  > BAAI/bge-small-en-v1.5 (384d, fast, English)
    BAAI/bge-base-en-v1.5 (768d, balanced)
    BAAI/bge-large-en-v1.5 (1024d, accurate)
    Custom (bring your own via API)
? Enable auto-linker? [Y/n]
? Auto-linker interval (seconds)? [60]
? Enable event ingest? (use arrow keys)
    None
  > File watcher
    Webhook endpoint
    NATS
    All of the above
? Pre-configure agent briefings? Enter agent IDs (comma-separated): [default]
? Enable HTTP debug server? [Y/n]

✅ Generated cortex.toml
✅ Created data directory
✅ Downloading embedding model... done (33MB)

Run `cortex serve` to start, or `cortex shell` for interactive mode.
```

---

## Client SDKs

Four official SDKs, all generated from the protobuf definition + hand-written convenience layers.

### Rust (native)

```rust
// cortex-client crate — thin wrapper over tonic-generated code
use cortex_client::CortexClient;

let client = CortexClient::connect("http://localhost:9090").await?;

let node = client.create_node(CreateNodeRequest {
    kind: "decision".into(),
    title: "Use Rust for performance-critical paths".into(),
    body: "Go for I/O-bound, Rust for CPU-bound.".into(),
    importance: 0.8,
    ..Default::default()
}).await?;

let results = client.search("language choices", 5).await?;
let briefing = client.briefing("kai").await?;
```

### Python

```python
# pip install cortex-memory
from cortex_memory import Cortex

# Server mode
cx = Cortex("localhost:9090")

# OR library mode (embedded, no server)
cx = Cortex.open("./memory.redb")

# Store
cx.store("decision", "Use FastAPI", body="Async + type hints", importance=0.8)

# Search
results = cx.search("backend choices", limit=5)
for r in results:
    print(f"{r.score:.2f} — {r.title}")

# Briefing
print(cx.briefing("my-agent"))

# Traverse
subgraph = cx.traverse(node_id, depth=3, direction="outgoing")
```

### TypeScript / Node

```typescript
// npm install @cortex-memory/client
import { Cortex } from '@cortex-memory/client';

const cx = new Cortex('localhost:9090');

await cx.store({
  kind: 'fact',
  title: 'API rate limit is 1000/min',
  tags: ['api', 'limits'],
});

const results = await cx.search('rate limits');
const briefing = await cx.briefing('my-agent');
```

### Go

```go
// go get github.com/MikeSquared-Agency/cortex-go
import "github.com/MikeSquared-Agency/cortex-go"

client, _ := cortex.Connect("localhost:9090")

node, _ := client.CreateNode(cortex.Node{
    Kind:  "event",
    Title: "Deployed v2.1 to production",
    Tags:  []string{"deploy", "production"},
})

results, _ := client.Search("deployment history", 10)
briefing, _ := client.Briefing("ops-agent")
```

---

## Access Control & Namespaces

Multi-agent setups need isolation. Agent A's private knowledge shouldn't leak to Agent B.

### Namespace Model

```toml
# cortex.toml
[access]
mode = "namespace"  # "open" (default) | "namespace" | "rbac"

[[access.namespaces]]
name = "kai"
agents = ["kai", "dutybound"]  # These agents can read/write this namespace
inherit = ["shared"]            # Also inherits from "shared" namespace

[[access.namespaces]]
name = "shared"
agents = ["*"]                  # Everyone can read
write = ["kai"]                 # Only kai can write
```

Implementation:
- Every node gets a `namespace` field (defaults to agent's namespace)
- Queries automatically filter by the requesting agent's allowed namespaces
- gRPC requests include an `agent_id` header for namespace resolution
- Briefings only pull from accessible namespaces

---

## Webhooks & Event Streaming

Push notifications when the graph changes.

```toml
# cortex.toml
[[webhooks]]
url = "https://myapp.com/hooks/cortex"
events = ["node.created", "edge.created", "contradiction.detected", "pattern.discovered"]
secret = "hmac-secret-here"

[[webhooks]]
url = "https://slack.com/webhook/..."
events = ["contradiction.detected"]  # Only contradictions
```

Event types:
- `node.created`, `node.updated`, `node.deleted`
- `edge.created`, `edge.decayed`, `edge.pruned`
- `contradiction.detected`
- `pattern.discovered` (auto-linker found a new pattern)
- `dedup.merged` (two nodes were merged)
- `briefing.generated`

Also: SSE (Server-Sent Events) endpoint at `GET /events/stream` for real-time UI.

---

## Observability

```toml
# cortex.toml
[observability]
prometheus = true           # Expose /metrics endpoint
prometheus_port = 9100
opentelemetry = false       # OTLP exporter
otlp_endpoint = "http://localhost:4317"
```

Metrics:
- `cortex_nodes_total` (gauge, by kind)
- `cortex_edges_total` (gauge, by relation)
- `cortex_db_size_bytes` (gauge)
- `cortex_autolinker_cycle_duration_seconds` (histogram)
- `cortex_autolinker_edges_created_total` (counter)
- `cortex_autolinker_edges_pruned_total` (counter)
- `cortex_search_duration_seconds` (histogram)
- `cortex_briefing_duration_seconds` (histogram)
- `cortex_briefing_cache_hit_ratio` (gauge)
- `cortex_ingest_events_total` (counter, by adapter)

---

## Schema Versioning & Migrations

The redb file includes a schema version in the META table.

```
META["schema_version"] = 1
```

On startup, Cortex checks the version:
- Same version → proceed
- Older version → run migrations automatically (with backup first)
- Newer version → refuse to open (downgrade not supported)

```
$ cortex migrate
Cortex data at ./data/cortex.redb
Current schema: v3
Target schema: v5

Migrations to apply:
  v3 → v4: Add namespace field to nodes
  v4 → v5: Add retention_policy to META

Creating backup at ./data/cortex.redb.v3.bak... done
Applying v3 → v4... done (0.3s, 12,459 nodes updated)
Applying v4 → v5... done (0.1s)

Schema upgraded to v5.
```

---

## Retention Policies

```toml
# cortex.toml
[retention]
default_ttl_days = 0        # 0 = keep forever

[retention.by_kind]
observation = 30             # Expire observations after 30 days
event = 90                   # Events after 90 days
decision = 0                 # Decisions kept forever
pattern = 0                  # Patterns kept forever

[retention.max_nodes]
limit = 100000               # Hard cap
strategy = "oldest_lowest_importance"  # What to evict when limit hit
```

The auto-linker runs retention checks alongside decay. Expired nodes are soft-deleted first, hard-deleted after a grace period (default 7 days).

---

## Import Adapters

Beyond file ingest, structured importers for common knowledge sources:

```
cortex import notes.md                    # Markdown file
cortex import --format json data.json     # JSON array of nodes
cortex import --format csv facts.csv      # CSV with kind,title,body,tags columns
cortex import --format jsonl stream.jsonl # JSON Lines (streaming)
cortex import --format obsidian ~/vault/  # Obsidian vault (respects wikilinks as edges)
cortex import --format notion <export>    # Notion export (HTML/MD)
```

The Obsidian importer is the killer feature for early adoption — personal knowledge management users already have structured vaults. Cortex turns their notes into a live knowledge graph.

---

## Graph Visualisation

Standalone SPA served at `/viz` (or as a separate `cortex-viz` package):

- Force-directed graph layout (D3.js or Three.js for 3D)
- Nodes coloured by kind, sized by importance
- Edges show relation type, thickness = weight
- Click node → detail panel (title, body, metadata, connected nodes)
- Search bar → highlights matching nodes
- Time slider → show graph state at any point in time
- Filter panel → toggle kinds, relations, weight thresholds
- Export → PNG, SVG, JSON

For the cloud product, this becomes the dashboard. For OSS users, it's a debug tool.

---

## Audit Log

Every mutation is logged to an append-only audit table:

```rust
struct AuditEntry {
    timestamp: DateTime<Utc>,
    action: AuditAction,      // Created, Updated, Deleted, Merged, EdgeCreated, EdgeDecayed...
    target_id: Uuid,           // Node or Edge ID
    actor: String,             // Agent ID or "auto-linker" or "decay-engine"
    details: Option<String>,   // JSON diff or description
}
```

Queryable via CLI and API:
```
cortex audit --since 24h
cortex audit --node <id>        # History of a specific node
cortex audit --actor auto-linker --since 1h
```

---

## Plugin System

Extend Cortex without forking:

```toml
# cortex.toml
[[plugins]]
path = "./plugins/slack-ingest.wasm"    # WASM plugin
type = "ingest"

[[plugins]]
path = "./plugins/custom-linker.wasm"
type = "linker_rule"
```

Plugin types:
- **Ingest adapter** — custom event sources
- **Linker rule** — custom auto-linking logic
- **Briefing section** — custom section generator
- **Export format** — custom export formats
- **Classifier** — custom node kind classification for file ingest

Runtime: WASM (via wasmtime) for sandboxing. Plugins can't access the filesystem or network unless explicitly granted.

---

## Encryption at Rest

```toml
# cortex.toml
[security]
encryption = true
# Key from environment variable (never in config file)
# CORTEX_ENCRYPTION_KEY=<base64-encoded-256-bit-key>
```

Implementation: AES-256-GCM encryption of the redb file via a transparent wrapper layer. Key derived from env var using Argon2. Backup files are also encrypted.

---

## Testing Utilities

For SDK developers and CI pipelines:

```
cortex test-config                    # Validate cortex.toml
cortex test-connection                # Verify gRPC connectivity
cortex mock-server                    # Start a mock Cortex server (returns canned data)
cortex seed                           # Populate with sample data for testing
cortex benchmark                      # Run built-in performance benchmarks
```

Python SDK includes test fixtures:
```python
import pytest
from cortex_memory.testing import mock_cortex

@pytest.fixture
def cortex():
    with mock_cortex() as cx:
        yield cx

def test_my_agent(cortex):
    cortex.store("fact", "test data")
    assert len(cortex.search("test")) == 1
```

---

## Documentation

### Structure

```
docs/
├── getting-started/
│   ├── quickstart.md          # 5-minute setup
│   ├── installation.md        # All platforms
│   ├── first-agent.md         # Build a simple agent with Cortex memory
│   └── configuration.md       # cortex.toml reference
├── concepts/
│   ├── architecture.md        # How Cortex works internally
│   ├── graph-model.md         # Nodes, edges, kinds, relations
│   ├── auto-linker.md         # How relationships are discovered
│   ├── briefings.md           # How context synthesis works
│   ├── decay-and-memory.md    # How knowledge ages
│   └── hybrid-search.md       # Vector + graph retrieval
├── guides/
│   ├── langchain.md           # LangChain integration
│   ├── crewai.md              # CrewAI integration
│   ├── openclaw.md            # OpenClaw integration
│   ├── obsidian-import.md     # Import your Obsidian vault
│   ├── multi-agent.md         # Shared memory for agent teams
│   ├── production.md          # Running in production
│   └── migration.md           # Migrating from other memory solutions
├── reference/
│   ├── cli.md                 # CLI command reference
│   ├── grpc-api.md            # gRPC API reference
│   ├── http-api.md            # HTTP API reference
│   ├── python-sdk.md          # Python SDK reference
│   ├── typescript-sdk.md      # TypeScript SDK reference
│   ├── go-sdk.md              # Go SDK reference
│   ├── rust-sdk.md            # Rust library reference
│   └── config.md              # Configuration reference
├── contributing/
│   ├── CONTRIBUTING.md
│   ├── development.md         # Dev setup, running tests
│   ├── architecture.md        # Internal architecture for contributors
│   └── plugins.md             # Writing plugins
└── blog/
    └── why-we-built-cortex.md # Launch blog post
```

### README.md (rewrite for public)

```markdown
# Cortex

**Embedded graph memory for AI agents. One binary. One file. Zero dependencies.**

Cortex is a local knowledge graph that stores what your AI agents know,
automatically discovers relationships between knowledge, and synthesises
context briefings on demand. Think SQLite, but for agent memory.

## Why Cortex?

Your agent's memory shouldn't be a text file. It should be a living graph
that wires itself, forgets what's irrelevant, and tells your agent exactly
what it needs to know.

- **Graph-native** — typed nodes and edges, not just vectors
- **Auto-linking** — relationships discovered via embedding similarity
- **Decay** — unused knowledge fades, important knowledge persists
- **Briefings** — "what do I need to know?" → tailored context document
- **Hybrid search** — vector similarity × graph proximity
- **Embedded** — single file, no external dependencies
- **Fast** — Rust, HNSW index, mmap'd storage

## Quick Start

...
```

### Licensing

**MIT** for cortex-core and cortex-server. Permissive, no friction.

Optional: dual license MIT + Apache 2.0 (same as Rust ecosystem convention).

Warren-adapter and cloud components: proprietary or separate license.

---

## Revised Build Order (12 weeks)

> **Detailed specs for each phase:**
> - [Phase A: Core Decoupling](phase-a-core-decoupling.md)
> - [Phase B: CLI & Library Mode](phase-b-cli-library.md)
> - [Phase C: Access Control & Policies](phase-c-access-policies.md)
> - [Phase D: Client SDKs](phase-d-sdks.md)
> - [Phase E: Observability & Plugins](phase-e-observability-plugins.md)
> - [Phase F: Documentation & Launch](phase-f-docs-launch.md)

### Phase A: Core Decoupling (Week 1-2)
- [ ] NodeKind/Relation → string newtypes with registry
- [ ] Configuration file parser (cortex.toml)
- [ ] Extract Warren adapter to separate crate
- [ ] Ingest adapter trait + implementations
- [ ] Schema versioning in META table

### Phase B: CLI & Library Mode (Week 3-4)
- [ ] `cortex init` setup wizard
- [ ] All CLI commands (node, edge, search, traverse, briefing, import, export)
- [ ] `cortex shell` REPL
- [ ] `Cortex::open()` library mode API
- [ ] `cortex doctor` + `cortex stats`
- [ ] Backup/restore with encryption

### Phase C: Access Control & Policies (Week 5-6)
- [ ] Namespace model
- [ ] Retention policies
- [ ] Audit log
- [ ] Encryption at rest

### Phase D: SDKs (Week 7-8)
- [ ] Rust client crate (cortex-client)
- [ ] Python SDK (cortex-memory on PyPI)
- [ ] TypeScript SDK (@cortex-memory/client on npm)
- [ ] Go SDK
- [ ] Testing utilities + mock server

### Phase E: Observability & Plugins (Week 9-10)
- [ ] Prometheus metrics endpoint
- [ ] OpenTelemetry traces
- [ ] Webhook/SSE event streaming
- [ ] WASM plugin system
- [ ] Import adapters (Obsidian, Notion, CSV, JSONL)

### Phase F: Documentation & Launch (Week 11-12)
- [ ] Full documentation site
- [ ] README rewrite
- [ ] Architecture diagrams
- [ ] Integration guides (LangChain, CrewAI, OpenClaw)
- [ ] Graph visualisation SPA
- [ ] Publish: crates.io, Docker Hub, Homebrew, pip, npm
- [ ] Blog post + HN/Reddit launch
- [ ] CONTRIBUTING.md, LICENSE, CODE_OF_CONDUCT
