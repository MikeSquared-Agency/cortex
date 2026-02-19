pub mod api;
pub mod briefing;
pub mod error;
pub mod graph;
pub mod ingest;
pub mod kinds;
pub mod linker;
pub mod policies;
pub mod relations;
pub mod storage;
pub mod types;
pub mod vector;

pub use api::{Cortex, LibraryConfig};
pub use error::{CortexError, Result};
pub use graph::{
    GraphEngine, GraphEngineImpl, Path, PathRequest, PathResult, Subgraph, TraversalBudget,
    TraversalDirection, TraversalRequest, TraversalStrategy,
};
pub use linker::{
    AutoLinker, AutoLinkerConfig, AutoLinkerMetrics, Contradiction, ContradictionDetector,
    DecayConfig, DecayEngine, DedupAction, DedupScanner, DuplicatePair, LinkRule, ProposedEdge,
    Resolution, SimilarityLinkRule, StructuralRule,
};
pub use policies::{
    AuditAction, AuditEntry, AuditFilter, AuditLog, RetentionConfig, RetentionEngine,
    RetentionMaxNodes,
};
pub use storage::{NodeFilter, RedbStorage, Storage, StorageStats, CURRENT_SCHEMA_VERSION};
pub use types::*;
pub use vector::{
    embedding_input, EmbeddingService, FastEmbedService, HnswIndex, HybridQuery, HybridResult,
    HybridSearch, RwLockVectorIndex, SimilarityConfig, SimilarityResult, VectorFilter, VectorIndex,
};

#[cfg(test)]
mod tests;
