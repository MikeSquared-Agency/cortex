use cortex_core::*;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

/// Thin wrapper around WarrenNatsAdapter for backward compatibility.
pub struct NatsIngest {
    inner: warren_adapter::WarrenNatsAdapter,
}

impl NatsIngest {
    pub fn new(
        client: async_nats::Client,
        storage: Arc<RedbStorage>,
        embedding_service: Arc<FastEmbedService>,
        vector_index: Arc<StdRwLock<HnswIndex>>,
        graph_version: Arc<AtomicU64>,
    ) -> Self {
        Self {
            inner: warren_adapter::WarrenNatsAdapter::new(
                client,
                storage,
                embedding_service,
                vector_index,
                graph_version,
            ),
        }
    }

    pub async fn start(&self) -> Result<()> {
        self.inner.start().await
    }
}
