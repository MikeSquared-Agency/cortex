mod config;
mod embedding;
mod hybrid;
mod index;
mod scoring;

pub use config::SimilarityConfig;
pub use embedding::{embedding_input, EmbeddingService, FastEmbedService};
pub use hybrid::{HybridQuery, HybridResult, HybridSearch};
pub use index::{HnswIndex, RwLockVectorIndex, SimilarityResult, VectorFilter, VectorIndex};
pub use scoring::{apply_score_decay, ScoreDecayConfig};

#[cfg(test)]
mod tests;
