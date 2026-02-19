use async_nats::Client;
use async_trait::async_trait;
use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::CortexError;
use cortex_core::Result;
use futures::stream::{BoxStream, SelectAll, StreamExt};

/// Generic NATS adapter — subscribes to configured subjects and
/// expects messages to be JSON-encoded IngestEvent payloads.
pub struct NatsAdapter {
    pub url: String,
    pub subjects: Vec<String>,
}

#[async_trait]
impl IngestAdapter for NatsAdapter {
    fn name(&self) -> &str {
        "nats"
    }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        let client: Client = async_nats::connect(&self.url).await.map_err(|e| {
            CortexError::Validation(format!("NATS connect to {} failed: {}", self.url, e))
        })?;

        let mut merged: SelectAll<BoxStream<'static, IngestEvent>> = SelectAll::new();

        for subject in &self.subjects {
            let sub = client.subscribe(subject.clone()).await.map_err(|e| {
                CortexError::Validation(format!("NATS subscribe to '{}' failed: {}", subject, e))
            })?;

            let stream: BoxStream<'static, IngestEvent> = Box::pin(sub.filter_map(|msg| {
                let event = serde_json::from_slice::<IngestEvent>(&msg.payload).ok();
                if event.is_none() {
                    tracing::warn!("Failed to parse NATS message as IngestEvent");
                }
                async move { event }
            }));

            merged.push(stream);
        }

        Ok(Box::pin(merged))
    }
}
