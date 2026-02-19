# Phase A — Core Decoupling

**Duration:** 2 weeks  
**Goal:** Transform Cortex from Warren-internal to framework-agnostic. Every Warren-specific decision becomes configurable.

---

## A1. Configurable Node Kinds

### Current State
`NodeKind` is a Rust enum with 8 hardcoded variants. Adding a kind requires a code change, recompile, and release.

### Target State
`NodeKind` becomes a newtype wrapper around `String`, validated against a configurable registry.

### Implementation

```rust
/// A validated node kind. Lowercase alphanumeric + hyphens, 1-64 chars.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeKind(String);

impl NodeKind {
    pub fn new(kind: &str) -> Result<Self> {
        let kind = kind.to_lowercase();
        if kind.is_empty() || kind.len() > 64 {
            return Err(CortexError::Validation("Kind must be 1-64 chars".into()));
        }
        if !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(CortexError::Validation("Kind must be alphanumeric + hyphens".into()));
        }
        Ok(Self(kind))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}
```

**Built-in constants** (convenience, not required):
```rust
impl NodeKind {
    pub fn agent() -> Self { Self("agent".into()) }
    pub fn decision() -> Self { Self("decision".into()) }
    pub fn fact() -> Self { Self("fact".into()) }
    pub fn event() -> Self { Self("event".into()) }
    pub fn goal() -> Self { Self("goal".into()) }
    pub fn preference() -> Self { Self("preference".into()) }
    pub fn pattern() -> Self { Self("pattern".into()) }
    pub fn observation() -> Self { Self("observation".into()) }
}
```

**Registry:**
```rust
pub struct KindRegistry {
    kinds: HashSet<NodeKind>,
    strict: bool,  // If true, reject unregistered kinds. If false, allow any.
}
```

**Config:**
```toml
[schema]
strict_kinds = false  # true = only allow registered kinds
node_kinds = [
    "agent", "decision", "fact", "event",
    "goal", "preference", "pattern", "observation"
]
```

**Migration:** Existing redb files store kinds as `u8`. Migration converts to string storage. Map: `0=agent, 1=decision, ...` Backward compatible — reads both formats.

### Storage Impact
- `NODES_BY_KIND` multimap changes key type from `u8` to `&str`
- Serialization: bincode still works (String is natively supported)
- Index rebuild on first open after migration (~1ms per 1000 nodes)

### Tests
- Create node with custom kind succeeds when `strict_kinds = false`
- Create node with unregistered kind fails when `strict_kinds = true`
- Validation rejects empty, too long, special characters
- Migration from u8 to string kinds preserves all data
- Built-in constants match string values
- Registry add/remove at runtime

---

## A2. Configurable Relations

Same pattern as A1. `Relation` enum → `Relation(String)` newtype.

```rust
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct Relation(String);

impl Relation {
    pub fn new(relation: &str) -> Result<Self> { /* same validation as NodeKind */ }
    
    // Built-in convenience constructors
    pub fn informed_by() -> Self { Self("informed-by".into()) }
    pub fn led_to() -> Self { Self("led-to".into()) }
    pub fn applies_to() -> Self { Self("applies-to".into()) }
    pub fn contradicts() -> Self { Self("contradicts".into()) }
    pub fn supersedes() -> Self { Self("supersedes".into()) }
    pub fn depends_on() -> Self { Self("depends-on".into()) }
    pub fn related_to() -> Self { Self("related-to".into()) }
    pub fn instance_of() -> Self { Self("instance-of".into()) }
}
```

**Config:**
```toml
[schema]
relations = [
    "informed-by", "led-to", "applies-to", "contradicts",
    "supersedes", "depends-on", "related-to", "instance-of"
]
```

### Tests
- Same set as A1 adapted for relations
- Auto-linker structural rules map to string relations correctly
- Contradiction detection uses configurable "contradicts" relation name

---

## A3. Configuration File (cortex.toml)

### Parser
Use `toml` crate + `serde` for deserialization into a typed `CortexConfig` struct.

```rust
#[derive(Debug, Deserialize)]
pub struct CortexConfig {
    pub server: ServerConfig,
    pub schema: SchemaConfig,
    pub embedding: EmbeddingConfig,
    pub auto_linker: AutoLinkerConfig,
    pub briefing: BriefingConfig,
    pub ingest: IngestConfig,
    pub access: AccessConfig,
    pub retention: RetentionConfig,
    pub security: SecurityConfig,
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_grpc_addr")]
    pub grpc_addr: String,        // "0.0.0.0:9090"
    #[serde(default = "default_http_addr")]
    pub http_addr: String,        // "0.0.0.0:9091"
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,        // "./data"
}

#[derive(Debug, Deserialize)]
pub struct SchemaConfig {
    #[serde(default)]
    pub strict_kinds: bool,
    #[serde(default = "default_node_kinds")]
    pub node_kinds: Vec<String>,
    #[serde(default = "default_relations")]
    pub relations: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_model")]
    pub model: String,           // "BAAI/bge-small-en-v1.5"
    pub dimension: Option<usize>, // Override for custom models
    pub api_url: Option<String>,  // For external embedding APIs
}
```

### Config Resolution Order
1. `cortex.toml` in working directory
2. `$CORTEX_CONFIG` environment variable (path to config file)
3. Individual `CORTEX_*` env vars override specific fields
4. CLI flags override everything

### Validation
`cortex config validate` checks:
- TOML syntax
- All referenced kinds exist in registry (if strict)
- All referenced relations exist
- Ports are valid
- Paths are writable
- Embedding model is recognized (or custom + dimension specified)

### Default Config
If no `cortex.toml` exists, Cortex uses sensible defaults. Runs out of the box with zero config.

### Tests
- Parse minimal config (empty file → all defaults)
- Parse full config with all fields
- Env var override works
- Invalid TOML → clear error message
- Missing required field with no default → clear error
- `cortex config validate` catches all error types
- Config hot-reload on SIGHUP (Linux) / file watch

---

## A4. Ingest Adapter Trait

### Trait Definition

```rust
#[async_trait]
pub trait IngestAdapter: Send + Sync + 'static {
    /// Human-readable name for logging.
    fn name(&self) -> &str;
    
    /// Start consuming events. Returns a receiver for the event stream.
    async fn start(&self) -> Result<mpsc::Receiver<IngestEvent>>;
    
    /// Graceful shutdown.
    async fn stop(&self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct IngestEvent {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub tags: Vec<String>,
    pub source_agent: String,
    pub source_session: Option<String>,
    pub importance: Option<f32>,  // None = use default (0.5)
}
```

### Built-in Adapters

**WebhookAdapter** — HTTP POST endpoint:
```rust
pub struct WebhookAdapter {
    port: u16,
    auth_token: Option<String>,
}
```

Accepts POST `/ingest` with JSON body matching `IngestEvent`. Validates auth token if configured.

```toml
[ingest.webhook]
enabled = true
port = 9092
auth_token = "my-secret"  # Optional
```

**FileWatcherAdapter** — watches directory for new files:
```rust
pub struct FileWatcherAdapter {
    watch_dir: PathBuf,
    poll_interval: Duration,
}
```

Upgrades existing `FileIngest` to implement `IngestAdapter` trait. Uses `notify` crate for filesystem events (inotify on Linux, FSEvents on macOS).

```toml
[ingest.file]
watch_dir = "./data/ingest"
poll_interval_seconds = 5
```

**NatsAdapter** — configurable NATS consumer:
```rust
pub struct NatsAdapter {
    url: String,
    subjects: Vec<String>,
    mapping: HashMap<String, NatsEventMapping>,
}
```

```toml
[ingest.nats]
url = "nats://localhost:4222"
subjects = ["cortex.>"]

[[ingest.nats.mapping]]
subject = "cortex.decision"
kind = "decision"
title_field = "title"
body_field = "body"
```

**StdinAdapter** — reads JSON lines from stdin:
```rust
pub struct StdinAdapter;
```

For scripting: `cat events.jsonl | cortex serve --ingest-stdin`

### Warren Adapter (separate crate)

`cortex-warren-adapter` — maps Warren NATS subjects to Cortex events. NOT part of core. Optional dependency.

```toml
# Only if you're running Warren
[ingest.warren]
nats_url = "nats://hermes:4222"
```

### Adapter Manager

```rust
pub struct IngestManager {
    adapters: Vec<Box<dyn IngestAdapter>>,
    storage: Arc<dyn Storage>,
    embeddings: Arc<dyn EmbeddingService>,
    vector_index: Arc<RwLock<dyn VectorIndex>>,
}

impl IngestManager {
    pub async fn start_all(&self) -> Result<()> {
        for adapter in &self.adapters {
            let rx = adapter.start().await?;
            tokio::spawn(self.process_events(adapter.name(), rx));
        }
        Ok(())
    }
    
    async fn process_events(&self, name: &str, mut rx: mpsc::Receiver<IngestEvent>) {
        while let Some(event) = rx.recv().await {
            match self.ingest_event(event).await {
                Ok(node_id) => tracing::debug!("[{}] Ingested node {}", name, node_id),
                Err(e) => tracing::error!("[{}] Ingest failed: {}", name, e),
            }
        }
    }
}
```

### Tests
- WebhookAdapter accepts valid POST, rejects invalid auth
- FileWatcherAdapter picks up new .md/.txt files
- NatsAdapter maps subjects to events correctly
- StdinAdapter reads JSONL
- IngestManager starts/stops all adapters
- Events create nodes with correct kinds and metadata
- Adapter failure doesn't crash other adapters

---

## A5. Schema Versioning

### Version Header

```rust
const SCHEMA_VERSION: u32 = 2;  // Bump on any storage format change
const META_KEY_SCHEMA: &str = "schema_version";
```

On open:
```rust
fn check_schema(db: &Database) -> Result<()> {
    let read_txn = db.begin_read()?;
    let meta = read_txn.open_table(META)?;
    
    match meta.get("schema_version")? {
        None => {
            // Legacy database (v1) — needs migration
            drop(read_txn);
            migrate_v1_to_v2(db)?;
        }
        Some(bytes) => {
            let version: u32 = bincode::deserialize(bytes.value())?;
            match version.cmp(&SCHEMA_VERSION) {
                Ordering::Equal => {} // Current
                Ordering::Less => {
                    drop(read_txn);
                    run_migrations(db, version, SCHEMA_VERSION)?;
                }
                Ordering::Greater => {
                    return Err(CortexError::Validation(format!(
                        "Database schema v{} is newer than this binary (v{}). Upgrade Cortex.",
                        version, SCHEMA_VERSION
                    )));
                }
            }
        }
    }
    Ok(())
}
```

### Migration Framework

```rust
type MigrationFn = fn(&Database) -> Result<()>;

fn migrations() -> Vec<(u32, u32, MigrationFn)> {
    vec![
        (1, 2, migrate_v1_to_v2),  // enum kinds → string kinds
        // Future migrations added here
    ]
}

fn run_migrations(db: &Database, from: u32, to: u32) -> Result<()> {
    // Create backup first
    let backup_path = format!("{}.v{}.bak", db_path, from);
    std::fs::copy(db_path, &backup_path)?;
    
    for (from_v, to_v, migrate_fn) in migrations() {
        if from_v >= from && to_v <= to {
            tracing::info!("Applying migration v{} → v{}", from_v, to_v);
            migrate_fn(db)?;
        }
    }
    
    // Update version
    let write_txn = db.begin_write()?;
    let mut meta = write_txn.open_table(META)?;
    meta.insert("schema_version", bincode::serialize(&to)?)?;
    write_txn.commit()?;
    
    Ok(())
}
```

### Tests
- New database gets current schema version
- Legacy (no version) triggers v1→v2 migration
- Migration creates backup before modifying
- Newer schema version refuses to open with clear error
- Migration preserves all nodes and edges
- Multiple sequential migrations apply in order

---

## Deliverables

1. `NodeKind` and `Relation` as string newtypes with registries
2. `CortexConfig` with full TOML parser and env var overrides
3. `IngestAdapter` trait with 4 built-in adapters (webhook, file, NATS, stdin)
4. Warren adapter extracted to `cortex-warren-adapter` crate
5. Schema versioning with migration framework
6. `cortex config validate` command
7. All existing tests updated for string kinds/relations
8. Migration test: v1 (enum) → v2 (string) preserves data
