# Phase E — Observability & Plugins

**Duration:** 2 weeks  
**Dependencies:** Phase A-D complete  
**Goal:** Production-grade monitoring, event streaming, and extensibility.

---

## E1. Prometheus Metrics

### Configuration

```toml
[observability]
prometheus = true
prometheus_port = 9100  # Separate from HTTP debug server
```

### Metrics

Exposed at `GET /metrics` in Prometheus text format.

```rust
pub struct Metrics {
    // Gauges
    nodes_total: IntGaugeVec,          // Labels: kind
    edges_total: IntGaugeVec,          // Labels: relation
    db_size_bytes: IntGauge,
    vector_index_size: IntGauge,
    briefing_cache_hit_ratio: Gauge,
    autolinker_backlog: IntGauge,
    
    // Counters
    nodes_created_total: IntCounterVec,  // Labels: kind, source
    edges_created_total: IntCounterVec,  // Labels: relation, provenance
    edges_pruned_total: IntCounter,
    contradictions_found_total: IntCounter,
    duplicates_merged_total: IntCounter,
    searches_total: IntCounterVec,       // Labels: type (similarity/hybrid)
    briefings_generated_total: IntCounterVec, // Labels: agent_id
    ingest_events_total: IntCounterVec,  // Labels: adapter
    
    // Histograms
    search_duration_seconds: HistogramVec,    // Labels: type
    briefing_duration_seconds: Histogram,
    autolinker_cycle_seconds: Histogram,
    grpc_request_duration: HistogramVec,      // Labels: method
    ingest_processing_seconds: HistogramVec,  // Labels: adapter
}
```

### Implementation

Use `prometheus` crate. Register metrics at startup. Update in:
- Storage layer (node/edge counts on mutation)
- Auto-linker (cycle metrics after each cycle)
- gRPC handlers (request duration histogram)
- Briefing engine (generation time, cache hits)
- Ingest manager (event counts per adapter)

### Grafana Dashboard

Ship a `grafana-dashboard.json` in the repo that users can import. Panels:
- Graph growth over time (nodes/edges)
- Auto-linker activity (edges created/pruned per hour)
- Search latency percentiles
- Briefing cache hit rate
- Ingest throughput by adapter
- Contradiction and dedup rates

---

## E2. OpenTelemetry Tracing

### Configuration

```toml
[observability]
opentelemetry = true
otlp_endpoint = "http://localhost:4317"
service_name = "cortex"
```

### Instrumented Operations

```rust
#[tracing::instrument(skip(self))]
pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let span = tracing::info_span!("cortex.search", query = query, limit = limit);
    let _guard = span.enter();
    
    // Embed query
    let embed_span = tracing::info_span!("cortex.embed");
    let embedding = {
        let _g = embed_span.enter();
        self.embeddings.embed(query)?
    };
    
    // Search index
    let search_span = tracing::info_span!("cortex.vector_search", k = limit);
    let results = {
        let _g = search_span.enter();
        self.vectors.read()?.search(&embedding, limit, None)?
    };
    
    Ok(results)
}
```

Every operation gets a trace: search → embed → vector_search → node_fetch. Distributed tracing across gRPC calls (propagate trace context in metadata).

Use `tracing-opentelemetry` crate + `opentelemetry-otlp` exporter.

---

## E3. Webhooks & Event Streaming

### Webhook Configuration

```toml
[[webhooks]]
name = "my-app"
url = "https://myapp.com/hooks/cortex"
events = ["node.created", "contradiction.detected"]
secret = "hmac-secret"       # HMAC-SHA256 signature in X-Cortex-Signature header
retry_count = 3
retry_delay_seconds = 5
```

### Event Types

```rust
pub enum CortexEvent {
    NodeCreated { node: Node },
    NodeUpdated { node: Node, changes: Vec<String> },
    NodeDeleted { node_id: NodeId },
    EdgeCreated { edge: Edge },
    EdgeDecayed { edge_id: EdgeId, old_weight: f32, new_weight: f32 },
    EdgePruned { edge_id: EdgeId },
    ContradictionDetected { node_a: NodeId, node_b: NodeId, similarity: f32 },
    PatternDiscovered { node: Node, linked_to: Vec<NodeId> },
    DuplicateMerged { kept: NodeId, retired: NodeId },
    BriefingGenerated { agent_id: String, sections: usize },
}
```

### Webhook Delivery

```rust
pub struct WebhookManager {
    hooks: Vec<WebhookConfig>,
    client: reqwest::Client,
    retry_queue: VecDeque<PendingWebhook>,
}

impl WebhookManager {
    pub async fn emit(&self, event: CortexEvent) {
        let event_type = event.event_type();
        let payload = serde_json::to_string(&event)?;
        
        for hook in &self.hooks {
            if hook.events.contains(&event_type) {
                let signature = hmac_sha256(&hook.secret, &payload);
                self.send(hook, &payload, &signature).await;
            }
        }
    }
    
    async fn send(&self, hook: &WebhookConfig, payload: &str, signature: &str) {
        let resp = self.client
            .post(&hook.url)
            .header("Content-Type", "application/json")
            .header("X-Cortex-Signature", signature)
            .header("X-Cortex-Event", event_type)
            .body(payload.to_string())
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        
        match resp {
            Ok(r) if r.status().is_success() => {},
            _ => {
                // Queue for retry
                self.retry_queue.push_back(PendingWebhook { hook, payload, attempts: 1 });
            }
        }
    }
}
```

### Server-Sent Events (SSE)

For real-time UIs without webhooks:

```
GET /events/stream
Accept: text/event-stream

event: node.created
data: {"node_id":"018d5f2a-...","kind":"decision","title":"..."}

event: contradiction.detected
data: {"node_a":"...","node_b":"...","similarity":0.87}
```

Implementation: `axum::response::Sse` with broadcast channel. All events go through a central `EventBus` that fans out to webhooks + SSE.

---

## E4. WASM Plugin System

### Plugin Types

```rust
pub enum PluginType {
    IngestAdapter,   // Custom event sources
    LinkerRule,      // Custom auto-linking logic
    BriefingSection, // Custom briefing section generator
    Classifier,      // Custom node kind classification
    ExportFormat,    // Custom export formats
}
```

### Plugin Interface (WASM)

Plugins are compiled to WASM and loaded at runtime via `wasmtime`.

```rust
// Plugin must export these functions
#[no_mangle]
pub extern "C" fn plugin_name() -> *const c_char;
#[no_mangle]
pub extern "C" fn plugin_type() -> u32;  // Maps to PluginType
#[no_mangle]
pub extern "C" fn plugin_version() -> *const c_char;

// For LinkerRule plugins:
#[no_mangle]
pub extern "C" fn evaluate_rule(
    node_a_ptr: *const u8, node_a_len: u32,
    node_b_ptr: *const u8, node_b_len: u32,
    score: f32,
) -> *const u8;  // Returns JSON-encoded ProposedEdge or null
```

### Configuration

```toml
[[plugins]]
path = "./plugins/slack-ingest.wasm"
type = "ingest"
config = { workspace = "T0AENMCEXM4", channel = "C0AFQAXC5DX" }

[[plugins]]
path = "./plugins/custom-decay.wasm"
type = "linker_rule"
```

### Sandboxing

WASM plugins run in a sandbox:
- No filesystem access (unless explicitly granted)
- No network access (unless explicitly granted)
- Memory limit: 64MB per plugin
- Execution timeout: 5 seconds per call
- Capabilities declared in plugin manifest

### Plugin Registry

Future: `cortex plugin search`, `cortex plugin install <name>` — fetch from a central registry (clawhub.com or similar).

---

## E5. Import Adapters

### Obsidian Vault Importer

The killer feature for early adoption.

```rust
pub struct ObsidianImporter {
    vault_path: PathBuf,
}

impl ObsidianImporter {
    pub fn import(&self, storage: &dyn Storage) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        let mut node_map: HashMap<String, NodeId> = HashMap::new();
        
        // Pass 1: Create nodes from all .md files
        for entry in WalkDir::new(&self.vault_path) {
            let path = entry?.path();
            if path.extension() != Some("md".as_ref()) { continue; }
            
            let content = std::fs::read_to_string(path)?;
            let title = path.file_stem().unwrap().to_string_lossy();
            
            // Extract frontmatter (YAML between ---) for metadata
            let (frontmatter, body) = extract_frontmatter(&content);
            
            // Extract #tags from content
            let tags = extract_hashtags(&body);
            
            // Classify content
            let kind = classify_chunk(&body);
            
            let node = Node::new(kind, title.to_string(), body, ...);
            node.data.tags = tags;
            if let Some(fm) = frontmatter {
                node.data.metadata = fm;
            }
            
            storage.put_node(&node)?;
            node_map.insert(title.to_string(), node.id);
            stats.nodes += 1;
        }
        
        // Pass 2: Create edges from [[wikilinks]]
        for entry in WalkDir::new(&self.vault_path) {
            let path = entry?.path();
            if path.extension() != Some("md".as_ref()) { continue; }
            
            let content = std::fs::read_to_string(path)?;
            let title = path.file_stem().unwrap().to_string_lossy();
            let source_id = node_map[&*title];
            
            for link in extract_wikilinks(&content) {
                if let Some(&target_id) = node_map.get(&link) {
                    let edge = Edge::new(source_id, target_id, Relation::related_to(), 0.8, ...);
                    let _ = storage.put_edge(&edge);
                    stats.edges += 1;
                }
            }
        }
        
        Ok(stats)
    }
}
```

CLI: `cortex import --format obsidian ~/Documents/MyVault`

### Notion Importer

Reads a Notion export (HTML or Markdown):
```
cortex import --format notion ~/Downloads/Notion-Export/
```

Handles nested pages as hierarchy (creates `depends-on` edges for parent-child relationships).

### CSV Importer

```csv
kind,title,body,tags,importance
decision,"Use Rust","CPU-bound workload","architecture;rust",0.8
fact,"Port 8080","Server listens on 8080","infrastructure",0.5
```

```
cortex import --format csv data.csv
```

---

## Deliverables

1. Prometheus metrics endpoint (30+ metrics with Grafana dashboard JSON)
2. OpenTelemetry tracing on all operations
3. Webhook delivery with HMAC signing, retry, and backoff
4. SSE endpoint for real-time event streaming
5. WASM plugin system with sandboxing (wasmtime)
6. Obsidian vault importer (2-pass: nodes then wikilink edges)
7. Notion and CSV importers
8. Plugin configuration in cortex.toml
