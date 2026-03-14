pub mod api;
pub mod briefing;
pub mod error;
pub mod gate;
pub mod graph;
pub mod hooks;
pub mod ingest;
pub mod kinds;
pub mod linker;
pub mod policies;
pub mod prompt;
pub mod query;
pub mod relations;
pub mod storage;
pub mod types;
pub mod vector;

pub use api::{Cortex, LibraryConfig};
pub use error::{CortexError, Result};
pub use gate::schema::{FieldSchema, FieldType, KindSchema, SchemaValidator, SchemaViolation};
pub use gate::{
    GateCheck, GateRejection, GateResult, KindOverrideConfig, WriteGate, WriteGateConfig,
};
pub use graph::{
    GraphEngine, GraphEngineImpl, Path, PathRequest, PathResult, Subgraph, TraversalBudget,
    TraversalDirection, TraversalRequest, TraversalStrategy,
};
pub use hooks::{HookRegistry, MutationAction, MutationHook};
pub use linker::{
    AutoLinker, AutoLinkerConfig, AutoLinkerMetrics, Contradiction, ContradictionDetector,
    DecayConfig, DecayEngine, DedupAction, DedupScanner, DuplicatePair, LinkRule, ProposedEdge,
    Resolution, SimilarityLinkRule, StructuralRule,
};
pub use policies::{
    AuditAction, AuditEntry, AuditFilter, AuditLog, KindRetention, RetentionConfig,
    RetentionEngine, RetentionMaxNodes,
};
pub use query::{parse_and_compile as parse_filter, CmpOp, FieldFilter, FilterExpr};
pub use storage::{NodeFilter, RedbStorage, Storage, StorageStats, CURRENT_SCHEMA_VERSION};
pub use types::*;
pub use vector::{
    apply_score_decay, embedding_input, EmbeddingService, FastEmbedService, HnswIndex, HybridQuery,
    HybridResult, HybridSearch, RwLockVectorIndex, ScoreDecayConfig, SimilarityConfig,
    SimilarityResult, VectorFilter, VectorIndex,
};

#[cfg(test)]
mod tests;
