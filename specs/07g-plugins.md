> **Status:** DEFERRED — see GitHub issue #9

# Phase 7G: Plugin System

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires `IngestAdapter` trait, `CortexConfig` (plugins config block), string `NodeKind`/`Relation`.  
**Weeks:** 9–10 (can run in parallel with 7D/7E/7F)  

---

## Overview

A WASM-based plugin system using `wasmtime` for sandboxing. Plugins extend Cortex without requiring forks or recompilation. Five plugin types are supported: custom ingest adapters, linker rules, briefing section generators, export formats, and node classifiers. Plugins cannot access the filesystem or network unless explicitly granted via capability grants.

---

## Repository Layout

```
crates/cortex-server/src/
  plugins/
    mod.rs          — PluginManager: load + execute plugins
    loader.rs       — WASM module loading + linker setup
    sandbox.rs      — Capability grants + WASI config
    ingest.rs       — Ingest plugin adapter (implements IngestAdapter)
    linker_rule.rs  — Linker rule plugin (implements LinkRule)
    briefing.rs     — Briefing section plugin (implements BriefingSection)
    export.rs       — Export format plugin
    classifier.rs   — Classifier plugin
    host_api.rs     — Host functions exposed to plugins
```

---

## Configuration

```toml
# cortex.toml

[[plugins]]
path = "./plugins/slack-ingest.wasm"
type = "ingest"
# No grants — can only call host functions

[[plugins]]
path = "./plugins/custom-linker.wasm"
type = "linker_rule"

[[plugins]]
path = "./plugins/my-section.wasm"
type = "briefing_section"
[plugins.config]
section_name = "Customer Insights"
max_items = 5

[[plugins]]
path = "./plugins/csv-exporter.wasm"
type = "export_format"

[[plugins]]
path = "./plugins/topic-classifier.wasm"
type = "classifier"
# With network access (for remote model inference)
[plugins.grants]
network = true
```

Plugin types: `"ingest"` | `"linker_rule"` | `"briefing_section"` | `"export_format"` | `"classifier"`

---

## Task 1: Plugin Manager & WASM Loader

### File: `crates/cortex-server/src/plugins/mod.rs`

**Cargo.toml dependencies:**
```toml
wasmtime = "22"
wasmtime-wasi = "22"
```

```rust
use wasmtime::{Engine, Store, Module, Linker};
use wasmtime_wasi::WasiCtxBuilder;
use crate::config::PluginConfig;
use std::path::Path;

pub struct PluginManager {
    engine: Engine,
    plugins: Vec<LoadedPlugin>,
}

pub struct LoadedPlugin {
    pub config: PluginConfig,
    pub module: Module,
}

impl PluginManager {
    pub fn new() -> anyhow::Result<Self> {
        let engine = Engine::default();
        Ok(Self { engine, plugins: Vec::new() })
    }

    /// Load all plugins from config.
    pub fn load_all(&mut self, plugin_configs: &[PluginConfig]) -> anyhow::Result<()> {
        for config in plugin_configs {
            match self.load_plugin(config) {
                Ok(plugin) => {
                    tracing::info!("Loaded plugin: {} (type: {})", config.path.display(), config.r#type);
                    self.plugins.push(plugin);
                }
                Err(e) => {
                    tracing::error!("Failed to load plugin {}: {}", config.path.display(), e);
                    // Non-fatal: server continues without the plugin
                }
            }
        }
        Ok(())
    }

    fn load_plugin(&self, config: &PluginConfig) -> anyhow::Result<LoadedPlugin> {
        let module = Module::from_file(&self.engine, &config.path)?;
        Ok(LoadedPlugin { config: config.clone(), module })
    }

    /// Get all loaded ingest plugins.
    pub fn ingest_plugins(&self) -> impl Iterator<Item = &LoadedPlugin> {
        self.plugins.iter().filter(|p| p.config.r#type == "ingest")
    }

    /// Get all loaded linker rule plugins.
    pub fn linker_rule_plugins(&self) -> impl Iterator<Item = &LoadedPlugin> {
        self.plugins.iter().filter(|p| p.config.r#type == "linker_rule")
    }
}
```

---

## Task 2: Sandboxing

### File: `crates/cortex-server/src/plugins/sandbox.rs`

Plugins run in a WASI-enabled WASM sandbox. By default they have no filesystem or network access. Capabilities are granted explicitly in `cortex.toml`.

```rust
use wasmtime_wasi::{WasiCtxBuilder, WasiCtx};
use crate::config::PluginConfig;

/// Build a WASI context for a plugin, applying capability grants from config.
pub fn build_wasi_ctx(config: &PluginConfig) -> WasiCtx {
    let mut builder = WasiCtxBuilder::new();

    // Always: inherit stdout/stderr for logging
    builder.inherit_stdout().inherit_stderr();

    // Grants
    if let Some(ref grants) = config.grants {
        if grants.network.unwrap_or(false) {
            // Note: wasmtime-wasi doesn't expose raw TCP by default.
            // Network access via WASI sockets requires the socket capability.
            // Log a warning — full network is complex to sandbox properly.
            tracing::warn!(
                "Plugin {} requested network access. Ensure this is intentional.",
                config.path.display()
            );
        }
        if let Some(ref paths) = grants.fs_read {
            for path in paths {
                builder.preopened_dir(
                    std::fs::File::open(path).unwrap(),
                    path.to_str().unwrap()
                ).unwrap();
            }
        }
    }

    builder.build()
}
```

**Capability grants in config** (extend `PluginConfig` from Phase 7A):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginGrants {
    pub network: Option<bool>,
    pub fs_read: Option<Vec<std::path::PathBuf>>,
    pub fs_write: Option<Vec<std::path::PathBuf>>,
}

// In PluginConfig, add:
pub grants: Option<PluginGrants>,
pub config: Option<serde_json::Value>,   // Plugin-specific config, passed via env
```

---

## Task 3: Host API (Functions Exposed to Plugins)

### File: `crates/cortex-server/src/plugins/host_api.rs`

Plugins can call host functions to interact with Cortex. These are the only permitted Cortex operations — plugins cannot directly access redb.

**Host function signatures** (WASM ABI — i32 pointer/length pairs for strings):

```
// Logging
host_log(level: i32, msg_ptr: i32, msg_len: i32)

// Emitting ingest events (for ingest plugins)
host_emit_event(json_ptr: i32, json_len: i32) -> i32  // 0 = ok, -1 = error

// Reading graph data (for linker_rule and briefing plugins)
host_search_nodes(query_ptr: i32, query_len: i32, limit: i32, out_ptr: i32, out_len: i32) -> i32
host_get_node(id_ptr: i32, id_len: i32, out_ptr: i32, out_len: i32) -> i32

// Proposing edges (for linker_rule plugins)
host_propose_edge(json_ptr: i32, json_len: i32) -> i32

// Outputting briefing content (for briefing_section plugins)
host_emit_section(json_ptr: i32, json_len: i32) -> i32
```

**Linking host functions into the WASM module:**

```rust
use wasmtime::{Linker, Store};

pub fn link_host_functions<T>(linker: &mut Linker<T>) -> anyhow::Result<()>
where T: 'static {
    // Logging
    linker.func_wrap("cortex", "host_log", |level: i32, msg_ptr: i32, msg_len: i32| {
        // Read msg from plugin memory, log to tracing
    })?;

    // Emit ingest event
    linker.func_wrap("cortex", "host_emit_event", |ptr: i32, len: i32| -> i32 {
        // Read JSON from plugin memory, parse as IngestEvent, push to ingest channel
        0
    })?;

    // Search nodes (read-only)
    linker.func_wrap("cortex", "host_search_nodes",
        |query_ptr: i32, query_len: i32, limit: i32, out_ptr: i32, out_len: i32| -> i32 {
            // Perform search, write JSON results to plugin memory at out_ptr
            0
        })?;

    Ok(())
}
```

---

## Task 4: Ingest Plugin Adapter

### File: `crates/cortex-server/src/plugins/ingest.rs`

An ingest plugin is a WASM module that produces `IngestEvent` JSON objects by calling `host_emit_event`. The plugin is called on a configurable interval or runs as a long-lived poll loop.

**Plugin contract (what the WASM module must export):**
```
// Required exports:
cortex_ingest_start() -> i32     // Called once on startup. Returns 0 on success.
cortex_ingest_poll() -> i32      // Called periodically. Emits events via host_emit_event(). Returns 0.
```

**Host-side adapter:**
```rust
use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio::sync::mpsc;
use wasmtime::{Store, Instance, Engine};

pub struct WasmIngestAdapter {
    name: String,
    plugin: super::LoadedPlugin,
    engine: Engine,
    tx: mpsc::Sender<IngestEvent>,
}

impl WasmIngestAdapter {
    pub fn new(plugin: super::LoadedPlugin, engine: Engine) -> (Self, mpsc::Receiver<IngestEvent>) {
        let (tx, rx) = mpsc::channel(256);
        let name = plugin.config.path
            .file_stem().and_then(|s| s.to_str()).unwrap_or("wasm-plugin").to_string();
        (Self { name, plugin, engine, tx }, rx)
    }
}

#[async_trait]
impl IngestAdapter for WasmIngestAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        let (tx_clone, rx) = (self.tx.clone(), /* receiver already moved */);
        // Spawn blocking task that calls cortex_ingest_poll() periodically
        tokio::task::spawn_blocking(move || {
            // Instantiate module, link host functions, call cortex_ingest_start()
            // Then loop: call cortex_ingest_poll() every 10s, collect emitted events
        });
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }
}
```

---

## Task 5: Linker Rule Plugin

### File: `crates/cortex-server/src/plugins/linker_rule.rs`

A linker rule plugin implements custom auto-linking logic. Called once per auto-linker cycle for each candidate node pair.

**Plugin contract:**
```
// Required exports:
cortex_link_init() -> i32   // One-time setup. Returns 0 on success.

// Called with two node JSON strings, returns a proposed edge JSON or empty string.
cortex_link_evaluate(
    node_a_ptr: i32, node_a_len: i32,
    node_b_ptr: i32, node_b_len: i32,
    out_ptr: i32, out_len: i32
) -> i32   // Returns bytes written to out_ptr, or 0 if no edge
```

**Proposed edge JSON format:**
```json
{
  "relation": "mentions",
  "weight": 0.75,
  "reason": "Node A mentions entity from Node B"
}
```

**Host-side integration** (in `crates/cortex-core/src/linker/rules.rs`):

Plugin linker rules are loaded as `LinkRule` implementations:

```rust
pub struct PluginLinkerRule {
    name: String,
    // WASM instance handle
}

impl LinkRule for PluginLinkerRule {
    fn evaluate(&self, a: &Node, b: &Node) -> Option<ProposedEdge> {
        // Serialise nodes to JSON
        // Call WASM cortex_link_evaluate()
        // Deserialise returned ProposedEdge if non-empty
        todo!()
    }
}
```

---

## Task 6: Briefing Section Plugin

### File: `crates/cortex-server/src/plugins/briefing.rs`

A briefing section plugin generates a custom section of the context briefing.

**Plugin contract:**
```
// Required exports:
cortex_section_name(out_ptr: i32, out_len: i32) -> i32  // Returns section name string
cortex_section_generate(
    agent_id_ptr: i32, agent_id_len: i32,
    out_ptr: i32, out_len: i32
) -> i32   // Returns markdown string for this section
```

The plugin uses `host_search_nodes` and `host_get_node` to query the graph and build its section content.

**Host-side integration:**

Plugin briefing sections are registered with the `BriefingEngine`:

```rust
// In crates/cortex-core/src/briefing/engine.rs
// After loading built-in sections, add plugin sections:
for plugin in plugin_manager.briefing_section_plugins() {
    let section = WasmBriefingSection::new(plugin);
    engine.add_section(Box::new(section));
}
```

---

## Task 7: Export Format Plugin

### File: `crates/cortex-server/src/plugins/export.rs`

A custom export format plugin. Called with the full node/edge list; produces a byte stream.

**Plugin contract:**
```
// Required exports:
cortex_export_format_name(out_ptr: i32, out_len: i32) -> i32   // Returns format name (e.g. "graphml")

cortex_export_begin() -> i32        // Called once before nodes/edges
cortex_export_node(
    node_json_ptr: i32, node_json_len: i32
) -> i32                            // Called for each node; writes output via host function
cortex_export_edge(
    edge_json_ptr: i32, edge_json_len: i32
) -> i32                            // Called for each edge
cortex_export_end() -> i32          // Called after all nodes and edges
```

Output is emitted via `host_emit_export_chunk(data_ptr, data_len)`, which accumulates chunks into the final export output.

---

## Task 8: Classifier Plugin

### File: `crates/cortex-server/src/plugins/classifier.rs`

A classifier plugin assigns a `NodeKind` to an unclassified node (e.g., during file ingest when the kind is unknown).

**Plugin contract:**
```
// Required exports:
cortex_classify(
    title_ptr: i32, title_len: i32,
    body_ptr: i32, body_len: i32,
    out_ptr: i32, out_len: i32
) -> i32   // Returns kind string (e.g. "fact"), or 0 for "don't know"
```

**Host-side integration** (in file ingest / markdown importer):

```rust
// If kind is unknown during import, ask classifier plugins:
for plugin in plugin_manager.classifier_plugins() {
    if let Some(kind_str) = plugin.classify(&title, &body)? {
        return NodeKind::new(&kind_str);
    }
}
// Fall back to default kind
```

---

## Plugin SDK (for Plugin Developers)

To make plugin development easier, a minimal Rust plugin SDK is provided:

### `crates/cortex-plugin-sdk/` (new crate, not published to crates.io initially)

```rust
// cortex-plugin-sdk — helpers for writing Cortex plugins in Rust targeting WASM

/// Log a message to the host (tracing::info! equivalent).
pub fn log(level: LogLevel, msg: &str) {
    unsafe { host_log(level as i32, msg.as_ptr() as i32, msg.len() as i32); }
}

/// Emit an ingest event (for ingest plugins).
pub fn emit_event(event: &IngestEvent) -> Result<(), PluginError> {
    let json = serde_json::to_string(event)?;
    let result = unsafe { host_emit_event(json.as_ptr() as i32, json.len() as i32) };
    if result != 0 { Err(PluginError::HostError) } else { Ok(()) }
}

/// Search nodes (for linker_rule and briefing plugins).
pub fn search_nodes(query: &str, limit: usize) -> Vec<PluginNode> {
    // Allocate output buffer, call host_search_nodes, parse JSON response
    todo!()
}

extern "C" {
    fn host_log(level: i32, msg_ptr: i32, msg_len: i32);
    fn host_emit_event(ptr: i32, len: i32) -> i32;
    fn host_search_nodes(query_ptr: i32, query_len: i32, limit: i32, out_ptr: i32, out_len: i32) -> i32;
    fn host_get_node(id_ptr: i32, id_len: i32, out_ptr: i32, out_len: i32) -> i32;
    fn host_propose_edge(json_ptr: i32, json_len: i32) -> i32;
}
```

**Example ingest plugin (Rust → WASM):**

```rust
// examples/slack-ingest-plugin/src/lib.rs
use cortex_plugin_sdk::{emit_event, IngestEvent, log, LogLevel};

#[no_mangle]
pub extern "C" fn cortex_ingest_start() -> i32 {
    log(LogLevel::Info, "Slack ingest plugin started");
    0
}

#[no_mangle]
pub extern "C" fn cortex_ingest_poll() -> i32 {
    // Read from Slack API (if network grant is present), emit events
    let event = IngestEvent {
        kind: "event".into(),
        title: "Slack message from #general".into(),
        body: "Example message content".into(),
        tags: vec!["slack".into()],
        source: "slack-ingest-plugin".into(),
        ..Default::default()
    };
    emit_event(&event).unwrap();
    0
}
```

**Build target:** `wasm32-wasi`

```
cargo build --target wasm32-wasi --release
# Output: target/wasm32-wasi/release/slack_ingest_plugin.wasm
```

---

## Definition of Done

- [ ] `wasmtime` crate added to `cortex-server` dependencies and compiles
- [ ] `PluginManager::load_all()` loads `.wasm` files from configured `[[plugins]]` paths
- [ ] Invalid WASM files log an error but do not crash the server
- [ ] Plugins with no grants cannot access the host filesystem
- [ ] Plugin with `grants.network = true` is logged with a warning on startup
- [ ] An ingest plugin that calls `host_emit_event()` produces nodes in the graph
- [ ] The `WasmIngestAdapter` implements `IngestAdapter` and is included in the ingest pipeline
- [ ] A linker rule plugin that calls `cortex_link_evaluate()` can propose custom edges
- [ ] Plugin linker rules are invoked during each auto-linker cycle
- [ ] A briefing section plugin produces a section rendered in the briefing output
- [ ] An export format plugin is invoked by `cortex export --format <plugin-format-name>`
- [ ] A classifier plugin is invoked for untyped nodes during file/markdown import
- [ ] `cortex-plugin-sdk` crate compiles targeting `wasm32-wasi`
- [ ] Example plugin (in `examples/` or `crates/cortex-plugin-sdk/examples/`) builds and runs
- [ ] Plugin `host_log()` calls appear in the server's tracing output
- [ ] Multiple plugins of the same type can be loaded simultaneously
- [ ] Plugins loaded from relative paths are resolved relative to the `cortex.toml` directory
- [ ] `cargo test --workspace` passes with plugin system compiled in
