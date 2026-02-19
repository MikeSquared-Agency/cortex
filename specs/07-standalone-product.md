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
