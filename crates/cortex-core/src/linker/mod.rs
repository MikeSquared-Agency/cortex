//! Auto-Linker: Self-organizing graph through automatic edge discovery
//!
//! The auto-linker runs as a background process that:
//! - Scans for new/updated nodes
//! - Discovers relationships via embedding similarity and structural rules
//! - Creates edges automatically
//! - Applies decay to aging edges
//! - Detects and merges duplicate nodes
//! - Flags contradictions for review

mod auto_linker;
mod config;
mod decay;
mod dedup;
mod metrics;
mod rules;

#[cfg(test)]
mod tests;

pub use auto_linker::AutoLinker;
pub use config::{AutoLinkerConfig, DecayConfig};
pub use decay::DecayEngine;
pub use dedup::{DedupAction, DedupScanner, DuplicatePair};
pub use metrics::AutoLinkerMetrics;
pub use rules::{
    Contradiction, ContradictionDetector, LinkRule, ProposedEdge, Resolution, SimilarityLinkRule,
    StructuralRule,
};
