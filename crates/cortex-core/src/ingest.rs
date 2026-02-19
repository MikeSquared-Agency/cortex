use crate::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;
use std::collections::HashMap;

/// A normalised event from any ingest source.
/// Adapters convert their native event format into this.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
