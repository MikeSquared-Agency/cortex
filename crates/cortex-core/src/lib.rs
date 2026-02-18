pub mod types;
pub mod storage;
pub mod error;
pub mod graph;
pub mod vector;
pub mod linker;

pub use error::{CortexError, Result};
pub use types::*;
pub use storage::{Storage, NodeFilter, StorageStats, RedbStorage};
pub use graph::{
    GraphEngine, GraphEngineImpl, Subgraph, TraversalRequest, TraversalDirection,
    TraversalStrategy, PathRequest, PathResult, Path, TraversalBudget,
};
pub use vector::{
    EmbeddingService, FastEmbedService, VectorIndex, HnswIndex, SimilarityResult,
    VectorFilter, HybridQuery, HybridResult, HybridSearch, SimilarityConfig, embedding_input,
};
pub use linker::{
    AutoLinker, AutoLinkerConfig, AutoLinkerMetrics, DecayConfig, DecayEngine,
    DedupScanner, DedupAction, DuplicatePair, LinkRule, ProposedEdge,
    SimilarityLinkRule, StructuralRule, Contradiction, ContradictionDetector, Resolution,
};

#[cfg(test)]
mod tests;
