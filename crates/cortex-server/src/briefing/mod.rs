use cortex_core::briefing::BriefingEngine;
use cortex_core::{EmbeddingService, GraphEngine, Storage, VectorIndex};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// Background task: pre-warms briefing cache for known agents.
pub struct BriefingPrecomputer<S, E, V, G>
where
    S: Storage + 'static,
    E: EmbeddingService + Clone + Send + Sync + 'static,
    V: VectorIndex + Clone + Send + Sync + 'static,
    G: GraphEngine + Clone + Send + Sync + 'static,
{
    engine: Arc<BriefingEngine<S, E, V, G>>,
    agents: Vec<String>,
    interval: Duration,
}

impl<S, E, V, G> BriefingPrecomputer<S, E, V, G>
where
    S: Storage + 'static,
    E: EmbeddingService + Clone + Send + Sync + 'static,
    V: VectorIndex + Clone + Send + Sync + 'static,
    G: GraphEngine + Clone + Send + Sync + 'static,
{
    pub fn new(
        engine: Arc<BriefingEngine<S, E, V, G>>,
        agents: Vec<String>,
        interval: Duration,
    ) -> Self {
        Self {
            engine,
            agents,
            interval,
        }
    }

    /// Run the pre-computation loop. Call via `tokio::spawn`.
    pub async fn run(self) {
        info!(
            "BriefingPrecomputer started for agents: {:?}",
            self.agents
        );
        loop {
            for agent_id in &self.agents {
                match self.engine.generate(agent_id) {
                    Ok(b) => {
                        info!(
                            "Pre-computed briefing for '{}': {} sections, cached={}",
                            agent_id,
                            b.sections.len(),
                            b.cached
                        );
                    }
                    Err(e) => {
                        error!("Failed to pre-compute briefing for '{}': {}", agent_id, e);
                    }
                }
            }
            tokio::time::sleep(self.interval).await;
        }
    }
}
