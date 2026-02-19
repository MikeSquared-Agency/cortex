use super::{parse_subject, WarrenEvent};
use async_nats::Client;
use cortex_core::*;
use futures::StreamExt;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

pub struct NatsIngest {
    client: Client,
    storage: Arc<RedbStorage>,
    embedding_service: Arc<FastEmbedService>,
    vector_index: Arc<StdRwLock<HnswIndex>>,
}

impl NatsIngest {
    pub fn new(
        client: Client,
        storage: Arc<RedbStorage>,
        embedding_service: Arc<FastEmbedService>,
        vector_index: Arc<StdRwLock<HnswIndex>>,
    ) -> Self {
        Self {
            client,
            storage,
            embedding_service,
            vector_index,
        }
    }

    /// Start consuming Warren events
    pub async fn start(&self) -> Result<()> {
        let mut subscriber = self
            .client
            .subscribe("warren.>".to_string())
            .await
            .map_err(|e| CortexError::Validation(format!("NATS subscribe failed: {}", e)))?;

        tracing::info!("NATS consumer started, subscribed to warren.>");

        while let Some(msg) = subscriber.next().await {
            if let Err(e) = self.handle_message(msg).await {
                tracing::error!("Failed to handle NATS message: {}", e);
            }
        }

        Ok(())
    }

    async fn handle_message(&self, msg: async_nats::Message) -> Result<()> {
        let subject_type = parse_subject(&msg.subject);
        if subject_type.is_none() {
            return Ok(()); // Not a warren event
        }

        // Parse event
        let event: WarrenEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| CortexError::Validation(format!("Invalid event JSON: {}", e)))?;

        tracing::debug!("Received Warren event: {:?}", event);

        // Convert to node
        let mut node = event.to_node("warren");

        // Check for duplicates by title + source
        let existing = self.storage.list_nodes(
            NodeFilter::new()
                .with_source_agent(node.source.agent.clone())
                .with_limit(100),
        )?;

        let duplicate = existing.iter().any(|n| {
            n.data.title == node.data.title && n.source.session == node.source.session
        });

        if duplicate {
            tracing::debug!("Skipping duplicate event: {}", node.data.title);
            return Ok(());
        }

        // Generate embedding
        let text = embedding_input(&node);
        let embedding = self.embedding_service.embed(&text)?;
        node.embedding = Some(embedding.clone());

        // Store node
        self.storage.put_node(&node)?;

        // Index embedding
        {
            let mut index = self.vector_index.write().unwrap();
            index.insert(node.id, &embedding)?;
        }

        tracing::info!("Ingested Warren event as node: {}", node.id);

        Ok(())
    }
}
