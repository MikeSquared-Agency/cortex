> **Status:** IMPLEMENTED

# Phase 7A: Core Decoupling

**Status:** BLOCKS all other phases — implement this first.  
**Dependencies:** None (this is the foundation).  
**Weeks:** 1–2  

---

## Overview

Transform the hardcoded `NodeKind` enum and `Relation` enum into flexible string newtypes, introduce a `cortex.toml` configuration file, extract the Warren-specific NATS adapter into a separate optional crate, define an `IngestAdapter` trait for pluggable event sources, and add schema versioning to the redb storage layer.

Nothing user-facing changes in this phase — the goal is to remove the hardcoded assumptions that prevent Cortex from being used outside Warren.

---

## Current State

```
crates/
  cortex-core/       — core library (types, storage, graph, linker, briefing, vector)
  cortex-server/     — binary (gRPC, HTTP, NATS ingest, config)
  cortex-proto/      — protobuf definitions
```

Current `NodeKind` is a Rust enum with 8 variants. Current `Relation` is a Rust enum with 8 variants. The NATS ingest (`crates/cortex-server/src/nats/`) is hardcoded to subscribe to `warren.>` subjects and uses Warren-specific event shapes. Configuration is handled by `clap` CLI flags only (`crates/cortex-server/src/config.rs`).

---

## Target State

```
crates/
  cortex-core/       — core library, no Warren dependencies
  cortex-server/     — binary, reads cortex.toml, uses IngestAdapter trait
  cortex-proto/      — unchanged
  warren-adapter/    — optional crate: Warren NATS mapping (separate, not published to crates.io)
```

---

## Task 1: NodeKind → String Newtype

### File: `crates/cortex-core/src/types.rs`

Replace the `NodeKind` enum with a validated string newtype.

**Remove:**
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Agent, Decision, Fact, Event, Goal, Preference, Pattern, Observation,
}

impl NodeKind {
    pub fn to_u8(self) -> u8 { ... }
    pub fn from_u8(v: u8) -> Option<Self> { ... }
}
```

**Add:**
```rust
/// A node kind identifier. Lowercase alphanumeric + hyphens only.
/// Validated on construction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeKind(String);

impl NodeKind {
    pub fn new(kind: &str) -> Result<Self> {
        if kind.is_empty() {
            return Err(CortexError::Validation("NodeKind cannot be empty".into()));
        }
        if !kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(CortexError::Validation(
                format!("NodeKind '{}' must be lowercase alphanumeric + hyphens only", kind)
            ));
        }
        Ok(NodeKind(kind.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&str> for NodeKind {
    type Error = CortexError;
    fn try_from(s: &str) -> Result<Self> {
        NodeKind::new(s)
    }
}

impl TryFrom<String> for NodeKind {
    type Error = CortexError;
    fn try_from(s: String) -> Result<Self> {
        NodeKind::new(&s)
    }
}
```

**Built-in constants** (replace old enum variants — placed in a `kinds` module):

### File: `crates/cortex-core/src/kinds.rs` (new file)

```rust
use crate::NodeKind;

/// The 8 default node kinds shipped with Cortex.
/// Users may define additional kinds in cortex.toml.
pub mod defaults {
    use super::*;

    pub fn agent()       -> NodeKind { NodeKind::new("agent").unwrap() }
    pub fn decision()    -> NodeKind { NodeKind::new("decision").unwrap() }
    pub fn fact()        -> NodeKind { NodeKind::new("fact").unwrap() }
    pub fn event()       -> NodeKind { NodeKind::new("event").unwrap() }
    pub fn goal()        -> NodeKind { NodeKind::new("goal").unwrap() }
    pub fn preference()  -> NodeKind { NodeKind::new("preference").unwrap() }
    pub fn pattern()     -> NodeKind { NodeKind::new("pattern").unwrap() }
    pub fn observation() -> NodeKind { NodeKind::new("observation").unwrap() }

    pub fn all() -> Vec<NodeKind> {
        vec![agent(), decision(), fact(), event(), goal(), preference(), pattern(), observation()]
    }
}
```

### Storage compatibility

The storage layer currently stores `NodeKind` as a `u8`. After this change, store as a UTF-8 string in redb. The schema migration (Task 5) handles converting existing u8 values back to their string names.

In `crates/cortex-core/src/storage/redb_storage.rs`, update serialisation:
- Before: `node.kind.to_u8()` → stored as `u8`  
- After: `node.kind.as_str().as_bytes()` → stored as bytes (UTF-8 string)

---

## Task 2: Relation → String Newtype

### File: `crates/cortex-core/src/types.rs`

Same pattern as NodeKind.

**Remove:**
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Relation {
    InformedBy, LedTo, AppliesTo, Contradicts, Supersedes, DependsOn, RelatedTo, InstanceOf,
}
```

**Add:**
```rust
/// A relation type identifier. Lowercase alphanumeric + underscores only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Relation(String);

impl Relation {
    pub fn new(relation: &str) -> Result<Self> {
        if relation.is_empty() {
            return Err(CortexError::Validation("Relation cannot be empty".into()));
        }
        if !relation.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(CortexError::Validation(
                format!("Relation '{}' must be lowercase alphanumeric + underscores only", relation)
            ));
        }
        Ok(Relation(relation.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&str> for Relation {
    type Error = CortexError;
    fn try_from(s: &str) -> Result<Self> { Relation::new(s) }
}

impl TryFrom<String> for Relation {
    type Error = CortexError;
    fn try_from(s: String) -> Result<Self> { Relation::new(&s) }
}
```

**Built-in constants** (in the same `kinds.rs` or a new `relations.rs`):

### File: `crates/cortex-core/src/relations.rs` (new file)

```rust
use crate::Relation;

pub mod defaults {
    use super::*;

    pub fn informed_by() -> Relation { Relation::new("informed_by").unwrap() }
    pub fn led_to()       -> Relation { Relation::new("led_to").unwrap() }
    pub fn applies_to()   -> Relation { Relation::new("applies_to").unwrap() }
    pub fn contradicts()  -> Relation { Relation::new("contradicts").unwrap() }
    pub fn supersedes()   -> Relation { Relation::new("supersedes").unwrap() }
    pub fn depends_on()   -> Relation { Relation::new("depends_on").unwrap() }
    pub fn related_to()   -> Relation { Relation::new("related_to").unwrap() }
    pub fn instance_of()  -> Relation { Relation::new("instance_of").unwrap() }

    pub fn all() -> Vec<Relation> {
        vec![
            informed_by(), led_to(), applies_to(), contradicts(),
            supersedes(), depends_on(), related_to(), instance_of(),
        ]
    }
}
```

---

## Task 3: Configuration File (`cortex.toml`)

### File: `crates/cortex-server/src/config.rs` (rewrite)

Replace the `clap`-only config with a `cortex.toml` parser plus CLI flags (CLI flags override file values).

**New Cargo.toml dependency for cortex-server:**
```toml
[dependencies]
toml = "0.8"
serde = { version = "1", features = ["derive"] }
```

**Config struct hierarchy:**

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level config, parsed from cortex.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CortexConfig {
    pub server: ServerConfig,
    pub schema: SchemaConfig,
    pub embedding: EmbeddingConfig,
    pub auto_linker: AutoLinkerTomlConfig,
    pub briefing: BriefingConfig,
    pub ingest: IngestConfig,
    pub observability: ObservabilityConfig,
    pub retention: RetentionConfig,
    pub security: SecurityConfig,
    pub webhooks: Vec<WebhookConfig>,
    pub plugins: Vec<PluginConfig>,
    pub access: AccessConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub grpc_addr: String,
    pub http_addr: String,
    pub data_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:9090".into(),
            http_addr: "0.0.0.0:9091".into(),
            data_dir: PathBuf::from("./data"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    /// Registered node kinds. Defaults to the 8 built-in kinds.
    pub node_kinds: Vec<String>,
    /// Registered relation types. Defaults to the 8 built-in relations.
    pub relations: Vec<String>,
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            node_kinds: vec![
                "agent".into(), "decision".into(), "fact".into(), "event".into(),
                "goal".into(), "preference".into(), "pattern".into(), "observation".into(),
            ],
            relations: vec![
                "informed_by".into(), "led_to".into(), "applies_to".into(),
                "contradicts".into(), "supersedes".into(), "depends_on".into(),
                "related_to".into(), "instance_of".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model name. Passed to FastEmbed.
    pub model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self { model: "BAAI/bge-small-en-v1.5".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoLinkerTomlConfig {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub similarity_threshold: f32,
    pub dedup_threshold: f32,
    pub decay_rate_per_day: f32,
    pub max_edges_per_node: usize,
}

impl Default for AutoLinkerTomlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 60,
            similarity_threshold: 0.75,
            dedup_threshold: 0.92,
            decay_rate_per_day: 0.01,
            max_edges_per_node: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BriefingConfig {
    pub cache_ttl_seconds: u64,
    pub max_total_items: usize,
    pub max_chars: usize,
    pub precompute_agents: Vec<String>,
    pub sections: Vec<BriefingSectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefingSectionConfig {
    pub name: String,
    /// "filter" | "traversal" | "hybrid_search" | "contradictions"
    pub mode: String,
    pub query: Option<String>,
    pub max_items: Option<usize>,
    pub sort: Option<String>,
    pub vector_weight: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestConfig {
    pub nats: Option<NatsIngestConfig>,
    pub webhook: Option<WebhookIngestConfig>,
    pub file: Option<FileIngestConfig>,
    pub stdin: Option<StdinIngestConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsIngestConfig {
    pub url: String,
    /// Subjects to subscribe to (e.g. ["myapp.>"])
    pub subjects: Vec<String>,
}

impl Default for NatsIngestConfig {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".into(),
            subjects: vec!["cortex.>".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookIngestConfig {
    pub enabled: bool,
    pub port: u16,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileIngestConfig {
    pub watch_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StdinIngestConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservabilityConfig {
    pub prometheus: bool,
    pub prometheus_port: u16,
    pub opentelemetry: bool,
    pub otlp_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetentionConfig {
    pub default_ttl_days: u64,
    pub by_kind: std::collections::HashMap<String, u64>,
    pub max_nodes: Option<RetentionMaxNodes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionMaxNodes {
    pub limit: usize,
    pub strategy: String,  // "oldest_lowest_importance"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    pub encryption: bool,
    // Key comes from CORTEX_ENCRYPTION_KEY env var, never stored in config
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub events: Vec<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub path: PathBuf,
    /// "ingest" | "linker_rule" | "briefing_section" | "export_format" | "classifier"
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessConfig {
    /// "open" | "namespace" | "rbac"
    pub mode: String,
    pub namespaces: Vec<NamespaceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceConfig {
    pub name: String,
    pub agents: Vec<String>,
    pub inherit: Option<Vec<String>>,
    pub write: Option<Vec<String>>,
}
```

**Loading function:**

```rust
impl CortexConfig {
    /// Load from a cortex.toml file, merging with defaults.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: CortexConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from cortex.toml if it exists, otherwise use defaults.
    pub fn load_or_default(path: &std::path::Path) -> Self {
        if path.exists() {
            Self::load(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Validate the config. Returns a list of errors if invalid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for kind in &self.schema.node_kinds {
            if let Err(e) = cortex_core::NodeKind::new(kind) {
                errors.push(format!("schema.node_kinds: {}", e));
            }
        }
        for rel in &self.schema.relations {
            if let Err(e) = cortex_core::Relation::new(rel) {
                errors.push(format!("schema.relations: {}", e));
            }
        }
        errors
    }
}
```

**Example cortex.toml:**

```toml
[server]
grpc_addr = "0.0.0.0:9090"
http_addr = "0.0.0.0:9091"
data_dir = "./data"

[schema]
node_kinds = [
    "agent", "decision", "fact", "event",
    "goal", "preference", "pattern", "observation",
    # Custom kinds:
    "conversation", "document", "entity", "action",
]
relations = [
    "informed_by", "led_to", "applies_to", "contradicts",
    "supersedes", "depends_on", "related_to", "instance_of",
    # Custom relations:
    "mentions", "authored_by", "part_of",
]

[embedding]
model = "BAAI/bge-small-en-v1.5"

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
mode = "filter"
query = "kind:agent AND source:{agent_id}"
max_items = 5

[[briefing.sections]]
name = "Recent Activity"
mode = "filter"
query = "source:{agent_id} AND created_after:48h"
sort = "created_at:desc"
max_items = 10

[[briefing.sections]]
name = "Related Knowledge"
mode = "hybrid_search"
vector_weight = 0.7
max_items = 10

[ingest.file]
watch_dir = "./data/ingest"

[ingest.webhook]
enabled = false
port = 9092

[observability]
prometheus = true
prometheus_port = 9100
opentelemetry = false

[retention]
default_ttl_days = 0

[retention.by_kind]
observation = 30
event = 90

[security]
encryption = false

[access]
mode = "open"
```

---

## Task 4: Warren Adapter Extraction

### New crate: `crates/warren-adapter/`

Move all Warren-specific code out of `cortex-server` into a separate, optional crate.

**Directory structure:**
```
crates/warren-adapter/
  Cargo.toml
  src/
    lib.rs
    nats.rs       — Warren NATS subject parsing and event mapping
    types.rs      — WarrenEvent struct and conversion to cortex_core::Node
```

**`crates/warren-adapter/Cargo.toml`:**
```toml
[package]
name = "warren-adapter"
version = "0.1.0"
edition = "2021"
publish = false  # Internal crate, not published to crates.io

[dependencies]
cortex-core = { path = "../cortex-core" }
async-nats = "0.35"
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
```

**Move from `crates/cortex-server/src/nats/` to `crates/warren-adapter/src/`:**
- `nats/mod.rs` → `warren-adapter/src/nats.rs`
- `nats/ingest.rs` → logic merged into `warren-adapter/src/nats.rs`
- Warren event types (the `WarrenEvent` struct and `parse_subject`) → `warren-adapter/src/types.rs`

**`crates/warren-adapter/src/lib.rs`:**
```rust
pub mod nats;
pub mod types;

pub use nats::WarrenNatsAdapter;
pub use types::WarrenEvent;
```

**Update `crates/cortex-server/Cargo.toml`:**  
Add `warren-adapter` as an optional dependency behind a feature flag:
```toml
[dependencies]
warren-adapter = { path = "../crates/warren-adapter", optional = true }

[features]
warren = ["warren-adapter"]
default = ["warren"]  # On by default for backward compatibility during transition
```

---

## Task 5: IngestAdapter Trait

### File: `crates/cortex-core/src/ingest.rs` (new file)

Define the trait that all ingest adapters (NATS, webhook, file, stdin, warren) must implement.

```rust
use crate::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;
use std::collections::HashMap;

/// A normalised event from any ingest source.
/// Adapters convert their native event format into this.
#[derive(Debug, Clone)]
pub struct IngestEvent {
    /// Maps to NodeKind. Must match a registered kind.
    pub kind: String,
    /// Human-readable title (max 256 chars).
    pub title: String,
    /// Full content body.
    pub body: String,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, Value>,
    /// Tags for lightweight categorisation.
    pub tags: Vec<String>,
    /// Which adapter produced this event.
    pub source: String,
    /// Agent or session identifier.
    pub session: Option<String>,
    /// Importance score override (None = use default 0.5).
    pub importance: Option<f32>,
}

/// A pluggable ingest adapter.
/// Implementations subscribe to an event source and
/// emit a stream of IngestEvents for the core to process.
#[async_trait]
pub trait IngestAdapter: Send + Sync + 'static {
    /// Adapter name (used in tracing and metrics labels).
    fn name(&self) -> &str;

    /// Start producing events. Returns an async stream.
    /// The stream should run until cancelled or the source disconnects.
    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>>;
}
```

**Built-in adapter: Stdin** (simple, useful for scripting and testing):

### File: `crates/cortex-server/src/ingest/stdin.rs` (new file)

```rust
use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::Result;
use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct StdinAdapter;

#[async_trait]
impl IngestAdapter for StdinAdapter {
    fn name(&self) -> &str { "stdin" }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let lines = tokio_stream::wrappers::LinesStream::new(reader.lines());

        let s = lines.filter_map(|line| async move {
            let line = line.ok()?;
            serde_json::from_str::<IngestEvent>(&line).ok()
        });

        Ok(Box::pin(s))
    }
}
```

**Built-in adapter: NATS** (generic, not Warren-specific):

### File: `crates/cortex-server/src/ingest/nats.rs` (new file)

```rust
use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;

pub struct NatsAdapter {
    pub url: String,
    pub subjects: Vec<String>,
}

#[async_trait]
impl IngestAdapter for NatsAdapter {
    fn name(&self) -> &str { "nats" }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        // Connect to NATS, subscribe to configured subjects,
        // parse each message as an IngestEvent JSON.
        // (Implementation details omitted — same pattern as existing NatsIngest)
        todo!()
    }
}
```

**Update `crates/cortex-server/src/main.rs`** to:
1. Load `CortexConfig` from `cortex.toml` at startup
2. Instantiate adapters based on config
3. Pass adapters to the ingest pipeline

---

## Task 6: Schema Versioning

### File: `crates/cortex-core/src/storage/redb_storage.rs`

Add schema version tracking to the META table in redb.

**Current META table entries (if any):** None explicitly versioned.

**Add:**
```rust
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
// v1 = original (enum-based NodeKind stored as u8)
// v2 = string-based NodeKind/Relation stored as UTF-8

const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const SCHEMA_VERSION_KEY: &str = "schema_version";
```

**On database open:**
```rust
fn check_schema_version(db: &Database) -> Result<()> {
    let read_txn = db.begin_read()?;
    let version = {
        let table = read_txn.open_table(META_TABLE).ok();
        table.and_then(|t| {
            t.get(SCHEMA_VERSION_KEY).ok().flatten()
                .and_then(|v| {
                    std::str::from_utf8(v.value()).ok()
                        .and_then(|s| s.parse::<u32>().ok())
                })
        }).unwrap_or(1) // No version = v1
    };

    match version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(CortexError::Validation(
            format!("Database schema v{} is older than current v{}. Run `cortex migrate`.",
                version, CURRENT_SCHEMA_VERSION)
        )),
        std::cmp::Ordering::Greater => Err(CortexError::Validation(
            format!("Database schema v{} is newer than this binary v{}. Upgrade Cortex.",
                version, CURRENT_SCHEMA_VERSION)
        )),
    }
}
```

**Write version on new database creation:**
```rust
fn write_schema_version(db: &Database) -> Result<()> {
    let write_txn = db.begin_write()?;
    {
        let mut table = write_txn.open_table(META_TABLE)?;
        table.insert(SCHEMA_VERSION_KEY, CURRENT_SCHEMA_VERSION.to_string().as_bytes())?;
    }
    write_txn.commit()?;
    Ok(())
}
```

**Migration v1 → v2** (NodeKind u8 → string):

### File: `crates/cortex-server/src/migration/mod.rs` (extend existing)

```rust
/// Migrate schema v1 to v2:
/// NodeKind stored as u8 → stored as UTF-8 string.
pub fn migrate_v1_to_v2(db: &mut RedbStorage) -> anyhow::Result<()> {
    let u8_to_kind = |v: u8| -> &'static str {
        match v {
            0 => "agent",
            1 => "decision",
            2 => "fact",
            3 => "event",
            4 => "goal",
            5 => "preference",
            6 => "pattern",
            7 => "observation",
            _ => "unknown",
        }
    };

    // Read all nodes, update kind field, rewrite
    // Implementation depends on internal redb table structure
    tracing::info!("Migrating schema v1 → v2 (NodeKind u8 → string)...");
    // ... node iteration and rewrite ...
    tracing::info!("Migration v1 → v2 complete.");
    Ok(())
}
```

The CLI command `cortex migrate` (implemented in Phase 7B) calls these migration functions.

---

## Codebase Changes Summary

| File | Action |
|------|--------|
| `crates/cortex-core/src/types.rs` | Replace `NodeKind` enum + `Relation` enum with string newtypes |
| `crates/cortex-core/src/kinds.rs` | New — built-in kind constants |
| `crates/cortex-core/src/relations.rs` | New — built-in relation constants |
| `crates/cortex-core/src/ingest.rs` | New — `IngestAdapter` trait + `IngestEvent` struct |
| `crates/cortex-core/src/lib.rs` | Export new modules |
| `crates/cortex-core/src/storage/redb_storage.rs` | Schema versioning + string-based kind storage |
| `crates/cortex-server/src/config.rs` | Rewrite to `CortexConfig` toml struct |
| `crates/cortex-server/src/ingest/` | New directory with `nats.rs`, `stdin.rs` adapters |
| `crates/cortex-server/src/nats/` | Remove (content moved to warren-adapter) |
| `crates/warren-adapter/` | New crate — Warren NATS mapping |
| `crates/cortex-server/Cargo.toml` | Add `warren-adapter` as optional feature |
| `crates/cortex-server/src/migration/mod.rs` | Add `migrate_v1_to_v2` |

All protobuf definitions in `crates/cortex-proto/` are unchanged.

---

## Definition of Done

- [ ] `cargo test --workspace` passes with zero failures
- [ ] `NodeKind::new("agent")` succeeds; `NodeKind::new("Agent")` returns `Err`
- [ ] `NodeKind::new("my-custom-kind")` succeeds
- [ ] `Relation::new("mentions")` succeeds; `Relation::new("CamelCase")` returns `Err`
- [ ] `cortex.toml` is parsed correctly; missing fields use defaults
- [ ] `CortexConfig::validate()` returns errors for invalid kind/relation names
- [ ] Server starts successfully when `cortex.toml` is present
- [ ] Server starts successfully when `cortex.toml` is absent (uses defaults)
- [ ] Warren NATS adapter is in `crates/warren-adapter/`, not in `cortex-server`
- [ ] `cortex-server` compiles without the `warren` feature enabled
- [ ] `cortex-server` compiles with the `warren` feature enabled
- [ ] `IngestAdapter` trait is in `cortex-core::ingest`
- [ ] Stdin adapter implements `IngestAdapter`
- [ ] New redb database has `META["schema_version"] = "2"`
- [ ] Opening a v1 database without running `cortex migrate` returns a clear error
- [ ] `migrate_v1_to_v2` correctly converts all node kinds from u8 to string
- [ ] All existing integration tests in `crates/cortex-server/tests/integration_test.rs` pass
