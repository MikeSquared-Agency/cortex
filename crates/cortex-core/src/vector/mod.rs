mod embedding;
mod index;
mod hybrid;
mod config;

pub use embedding::{EmbeddingService, FastEmbedService, embedding_input};
pub use index::{VectorIndex, HnswIndex, SimilarityResult, VectorFilter};
pub use hybrid::{HybridQuery, HybridResult, HybridSearch};
pub use config::SimilarityConfig;

#[cfg(test)]
mod tests;
