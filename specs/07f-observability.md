> **Status:** DEFERRED — see GitHub issue #17

# Phase 7F: Observability

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires `CortexConfig` (observability/webhooks config blocks).  
**Weeks:** 9–10 (can run in parallel with 7D/7E/7G)  

---

## Overview

Expose runtime metrics via Prometheus, distributed traces via OpenTelemetry, push notifications to external systems via HMAC-signed webhooks, and real-time graph change events via a Server-Sent Events (SSE) stream. All features are opt-in via `cortex.toml`.

---

## Repository Layout

```
crates/cortex-server/src/
  observability/
    mod.rs          — ObservabilityManager: owns metrics registry, tracer, webhook dispatcher
    metrics.rs      — Prometheus metrics definitions (all gauge/counter/histogram names)
    tracing.rs      — OpenTelemetry tracer setup + span helpers
    webhooks.rs     — Webhook dispatcher (HMAC signing, HTTP POST, retry)
    sse.rs          — SSE event bus + /events/stream HTTP endpoint
```

---

## Task 1: Prometheus Metrics

### File: `crates/cortex-server/src/observability/metrics.rs`

**Cargo.toml dependencies:**
```toml
prometheus = "0.13"
```

**Configuration:**
```toml
[observability]
prometheus = true
prometheus_port = 9100     # Separate port for /metrics
opentelemetry = false
```

**All metric definitions:**

```rust
use prometheus::{
    register_gauge_vec, register_counter_vec, register_histogram_vec,
    GaugeVec, CounterVec, HistogramVec, Registry, Encoder, TextEncoder,
};
use once_cell::sync::Lazy;

// ─── Gauges ───────────────────────────────────────────────────────────────────

/// Total number of live (non-deleted) nodes, labelled by kind.
/// cortex_nodes_total{kind="fact"} 4231
pub static NODES_TOTAL: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "cortex_nodes_total",
        "Total number of live nodes by kind",
        &["kind"]
    ).unwrap()
});

/// Total number of edges, labelled by relation.
/// cortex_edges_total{relation="related_to"} 21234
pub static EDGES_TOTAL: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "cortex_edges_total",
        "Total number of edges by relation",
        &["relation"]
    ).unwrap()
});

/// Size of the redb file in bytes.
/// cortex_db_size_bytes 49545216
pub static DB_SIZE_BYTES: Lazy<prometheus::Gauge> = Lazy::new(|| {
    prometheus::register_gauge!(
        "cortex_db_size_bytes",
        "Size of the Cortex database file in bytes"
    ).unwrap()
});

/// Briefing cache hit ratio (0.0–1.0).
/// cortex_briefing_cache_hit_ratio 0.73
pub static BRIEFING_CACHE_HIT_RATIO: Lazy<prometheus::Gauge> = Lazy::new(|| {
    prometheus::register_gauge!(
        "cortex_briefing_cache_hit_ratio",
        "Ratio of briefing requests served from cache"
    ).unwrap()
});

// ─── Counters ─────────────────────────────────────────────────────────────────

/// Total edges created by the auto-linker (lifetime counter).
/// cortex_autolinker_edges_created_total 98712
pub static AUTOLINKER_EDGES_CREATED: Lazy<prometheus::Counter> = Lazy::new(|| {
    prometheus::register_counter!(
        "cortex_autolinker_edges_created_total",
        "Total edges created by the auto-linker since process start"
    ).unwrap()
});

/// Total edges pruned by the auto-linker (lifetime counter).
pub static AUTOLINKER_EDGES_PRUNED: Lazy<prometheus::Counter> = Lazy::new(|| {
    prometheus::register_counter!(
        "cortex_autolinker_edges_pruned_total",
        "Total edges pruned by the auto-linker since process start"
    ).unwrap()
});

/// Total ingest events processed, labelled by adapter name.
/// cortex_ingest_events_total{adapter="nats"} 5432
pub static INGEST_EVENTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "cortex_ingest_events_total",
        "Total ingest events processed by adapter",
        &["adapter"]
    ).unwrap()
});

// ─── Histograms ───────────────────────────────────────────────────────────────

/// Auto-linker cycle duration in seconds.
/// Buckets: 0.1, 0.5, 1, 5, 10, 30, 60, 120
pub static AUTOLINKER_CYCLE_DURATION: Lazy<prometheus::Histogram> = Lazy::new(|| {
    prometheus::register_histogram!(
        "cortex_autolinker_cycle_duration_seconds",
        "Duration of a complete auto-linker cycle",
        vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0]
    ).unwrap()
});

/// Search request duration in seconds, labelled by search type.
/// cortex_search_duration_seconds{type="semantic"} ...
pub static SEARCH_DURATION: Lazy<HistogramVec> = Lazy::new(|| {
    register_histogram_vec!(
        "cortex_search_duration_seconds",
        "Duration of search requests",
        &["type"],                         // "semantic" | "hybrid"
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    ).unwrap()
});

/// Briefing generation duration in seconds.
pub static BRIEFING_DURATION: Lazy<prometheus::Histogram> = Lazy::new(|| {
    prometheus::register_histogram!(
        "cortex_briefing_duration_seconds",
        "Duration of briefing generation requests",
        vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
    ).unwrap()
});
```

**Metrics HTTP server** (separate port from the main HTTP server):

```rust
pub async fn serve_metrics(port: u16) -> anyhow::Result<()> {
    use axum::{routing::get, Router};

    let app = Router::new().route("/metrics", get(metrics_handler));
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn metrics_handler() -> impl axum::response::IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        buffer,
    )
}
```

**Integration points** — update these locations to record metrics:

| Location | Metric |
|----------|--------|
| `AutoLinker::run_cycle()` | `AUTOLINKER_CYCLE_DURATION.observe(elapsed)` |
| `AutoLinker` — edge creation | `AUTOLINKER_EDGES_CREATED.inc()` |
| `AutoLinker` — edge pruning | `AUTOLINKER_EDGES_PRUNED.inc()` |
| gRPC `SearchNodes` handler | `SEARCH_DURATION.with_label_values(&["semantic"]).observe(elapsed)` |
| gRPC `SearchNodes` (hybrid) | `SEARCH_DURATION.with_label_values(&["hybrid"]).observe(elapsed)` |
| gRPC `GetBriefing` handler | `BRIEFING_DURATION.observe(elapsed)` |
| Ingest adapters | `INGEST_EVENTS_TOTAL.with_label_values(&[adapter_name]).inc()` |
| Background stats job (every 30s) | `NODES_TOTAL`, `EDGES_TOTAL`, `DB_SIZE_BYTES`, `BRIEFING_CACHE_HIT_RATIO` |

---

## Task 2: OpenTelemetry Traces

### File: `crates/cortex-server/src/observability/tracing.rs`

**Cargo.toml dependencies:**
```toml
opentelemetry = "0.23"
opentelemetry-otlp = "0.16"
tracing-opentelemetry = "0.24"
```

**Configuration:**
```toml
[observability]
opentelemetry = true
otlp_endpoint = "http://localhost:4317"   # Jaeger, Tempo, etc.
```

**Tracer setup:**
```rust
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracer(otlp_endpoint: &str) -> anyhow::Result<()> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint)
        )
        .with_trace_config(
            opentelemetry::sdk::trace::config()
                .with_resource(opentelemetry::sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", "cortex"),
                    opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                ]))
        )
        .install_batch(opentelemetry::runtime::Tokio)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Ok(())
}

pub fn shutdown_tracer() {
    global::shutdown_tracer_provider();
}
```

**Span instrumentation helpers:**

Key gRPC handlers and internal functions are wrapped with `#[tracing::instrument]` or manual spans. Target spans:

- `grpc.create_node` — CreateNode RPC (attributes: `kind`, `importance`)
- `grpc.search_nodes` — SearchNodes RPC (attributes: `query_len`, `limit`, `hybrid`)
- `grpc.get_briefing` — GetBriefing RPC (attributes: `agent_id`)
- `grpc.traverse` — Traverse RPC (attributes: `depth`)
- `auto_linker.cycle` — Full auto-linker cycle (attributes: `nodes_processed`, `edges_created`)
- `embedding.embed` — Embedding generation (attributes: `input_len`, `model`)
- `storage.put_node` — Database write
- `storage.list_nodes` — Database read (attributes: `filter_kind`)

---

## Task 3: Webhooks (HMAC-Signed)

### File: `crates/cortex-server/src/observability/webhooks.rs`

Push notifications to external HTTP endpoints when graph events occur.

**Configuration:**
```toml
[[webhooks]]
url = "https://myapp.com/hooks/cortex"
events = ["node.created", "edge.created", "contradiction.detected", "pattern.discovered"]
secret = "hmac-secret-here"

[[webhooks]]
url = "https://slack.com/webhook/..."
events = ["contradiction.detected"]
```

**Event types** (all possible values for the `events` field):
- `node.created`
- `node.updated`
- `node.deleted`
- `edge.created`
- `edge.decayed`
- `edge.pruned`
- `contradiction.detected`
- `pattern.discovered`  (auto-linker found a new pattern)
- `dedup.merged`        (two nodes were merged during dedup)
- `briefing.generated`

**Webhook payload structure:**
```json
{
  "event": "node.created",
  "timestamp": "2026-01-20T10:23:41Z",
  "data": {
    "node_id": "018e1234-abcd-7000-8000-000000000001",
    "kind": "decision",
    "title": "Use FastAPI for the backend",
    "actor": "kai"
  }
}
```

**HMAC-SHA256 signature** (header: `X-Cortex-Signature: sha256=<hex>`):

The signature is computed over the raw JSON body using the configured `secret`.

**Cargo.toml dependencies:**
```toml
hmac = "0.12"
sha2 = "0.10"
reqwest = { version = "0.12", features = ["json"] }
hex = "0.4"
```

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub event: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

pub struct WebhookDispatcher {
    configs: Vec<crate::config::WebhookConfig>,
    client: reqwest::Client,
}

impl WebhookDispatcher {
    pub fn new(configs: Vec<crate::config::WebhookConfig>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        Self { configs, client }
    }

    /// Dispatch an event to all matching webhooks.
    pub async fn dispatch(&self, event: WebhookEvent) {
        for config in &self.configs {
            if !config.events.contains(&event.event) && !config.events.contains(&"*".to_string()) {
                continue;
            }

            let body = serde_json::to_vec(&event).unwrap();
            let signature = if let Some(ref secret) = config.secret {
                Some(sign_payload(&body, secret))
            } else {
                None
            };

            let url = config.url.clone();
            let sig = signature.clone();
            let client = self.client.clone();
            let body_clone = body.clone();

            // Fire and forget — don't block the caller
            tokio::spawn(async move {
                let mut req = client.post(&url)
                    .header("Content-Type", "application/json")
                    .body(body_clone);

                if let Some(sig) = sig {
                    req = req.header("X-Cortex-Signature", format!("sha256={}", sig));
                }

                match req.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::debug!("Webhook delivered to {}: {}", url, resp.status());
                    }
                    Ok(resp) => {
                        tracing::warn!("Webhook to {} returned {}", url, resp.status());
                    }
                    Err(e) => {
                        tracing::error!("Webhook to {} failed: {}", url, e);
                    }
                }
            });
        }
    }
}

/// Compute HMAC-SHA256 signature over the payload.
/// Returns lowercase hex string.
fn sign_payload(body: &[u8], secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}
```

**Integration — emit webhook events from storage mutations:**

The `WebhookDispatcher` is passed to `RedbStorage` (or to the gRPC service layer) and called after successful writes:

```rust
// After successful put_node():
dispatcher.dispatch(WebhookEvent {
    event: "node.created".into(),
    timestamp: Utc::now(),
    data: serde_json::json!({
        "node_id": node.id,
        "kind": node.kind.as_str(),
        "title": node.data.title,
        "actor": node.source.agent,
    }),
}).await;
```

---

## Task 4: SSE Event Streaming

### File: `crates/cortex-server/src/observability/sse.rs`

Real-time graph changes delivered to browser/client via Server-Sent Events at `GET /events/stream`.

**Cargo.toml dependencies:**
```toml
axum = { version = "0.7", features = ["macros"] }
tokio-stream = "0.1"
```

```rust
use axum::{
    extract::State,
    response::{Sse, sse::Event},
};
use futures::stream::{self, Stream};
use std::convert::Infallible;
use tokio::sync::broadcast;

/// Global event bus. Sender lives in AppState; receivers created per SSE connection.
pub type EventBus = broadcast::Sender<SsePayload>;

#[derive(Debug, Clone)]
pub struct SsePayload {
    pub event_type: String,   // e.g. "node.created"
    pub data: String,          // JSON string
}

/// SSE handler: `GET /events/stream`
pub async fn sse_handler(
    State(bus): State<EventBus>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = bus.subscribe();

    let stream = tokio_stream::wrappers::BroadcastStream::new(receiver)
        .filter_map(|result| {
            std::future::ready(result.ok().map(|payload| {
                Ok(Event::default()
                    .event(payload.event_type)
                    .data(payload.data))
            }))
        });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("ping")
    )
}
```

**Publishing events to the SSE bus:**

The `EventBus` sender is stored in `AppState` and called alongside webhook dispatch:

```rust
// After successful put_node():
let _ = event_bus.send(SsePayload {
    event_type: "node.created".into(),
    data: serde_json::to_string(&serde_json::json!({
        "node_id": node.id,
        "kind": node.kind.as_str(),
        "title": node.data.title,
    })).unwrap(),
});
```

**HTTP route registration** (in `crates/cortex-server/src/http/routes.rs`):
```rust
router.route("/events/stream", get(sse::sse_handler))
```

**Client usage example:**
```javascript
const evtSource = new EventSource("http://localhost:9091/events/stream");

evtSource.addEventListener("node.created", (e) => {
  const node = JSON.parse(e.data);
  console.log("New node:", node.title);
});

evtSource.addEventListener("contradiction.detected", (e) => {
  console.warn("Contradiction:", e.data);
});
```

---

## Definition of Done

- [ ] `GET /metrics` returns Prometheus text exposition format when `prometheus = true`
- [ ] `/metrics` is served on `prometheus_port` (default 9100), not the main HTTP port
- [ ] `cortex_nodes_total{kind="fact"}` is present and accurate
- [ ] `cortex_edges_total{relation="related_to"}` is present and accurate
- [ ] `cortex_db_size_bytes` reflects the actual file size
- [ ] `cortex_autolinker_cycle_duration_seconds` is updated after each cycle
- [ ] `cortex_autolinker_edges_created_total` increments on each auto-linked edge
- [ ] `cortex_autolinker_edges_pruned_total` increments on each pruned edge
- [ ] `cortex_search_duration_seconds{type="semantic"}` is recorded on each search
- [ ] `cortex_search_duration_seconds{type="hybrid"}` is recorded on hybrid searches
- [ ] `cortex_briefing_duration_seconds` is recorded on each briefing generation
- [ ] `cortex_briefing_cache_hit_ratio` reflects the actual cache hit rate
- [ ] `cortex_ingest_events_total{adapter="nats"}` increments for each NATS-ingested event
- [ ] With `opentelemetry = true` and a valid `otlp_endpoint`, traces are exported
- [ ] Span `grpc.create_node` appears in the trace backend on node creation
- [ ] Span `auto_linker.cycle` appears in the trace backend on each linker cycle
- [ ] Server starts without error when `opentelemetry = false` (no OTLP connection attempted)
- [ ] Webhooks are dispatched to configured URLs on `node.created` events
- [ ] Webhook POST body is valid JSON matching the `WebhookEvent` schema
- [ ] `X-Cortex-Signature: sha256=<hex>` header is present when `secret` is configured
- [ ] HMAC signature can be verified with the configured secret
- [ ] Webhooks with event filters (`events = ["contradiction.detected"]`) only fire for matching events
- [ ] Webhook failures (non-2xx, network error) are logged but do not crash the server
- [ ] Webhook dispatch does not block the caller (fire and forget)
- [ ] `GET /events/stream` returns `Content-Type: text/event-stream`
- [ ] SSE stream delivers `node.created` events within 1 second of node creation
- [ ] SSE stream delivers `contradiction.detected` events from the auto-linker
- [ ] SSE stream sends keepalive `ping` every 30 seconds to prevent timeout
- [ ] Multiple simultaneous SSE clients all receive events
- [ ] Disconnected SSE clients do not cause memory leaks
- [ ] `cargo test --workspace` passes with observability features enabled
