pub mod cache;
pub mod engine;
pub mod ingest;
pub mod renderer;

pub use engine::{BriefingConfig, BriefingEngine, BriefingRoleConfig};

use chrono::{DateTime, Utc};

use crate::types::Node;

/// Controls how broadly the briefing engine scopes its traversals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BriefingScope {
    /// Default. Only the requesting agent's knowledge.
    #[default]
    Agent,
    /// Agent's knowledge plus a "Cross-agent context" section showing
    /// other agents' knowledge about entities the requesting agent references.
    Shared,
    /// Multi-agent unified briefing across the listed agent IDs.
    /// Not scoped to any single agent — used by orchestrators.
    Unified(Vec<String>),
}

/// A synthesised context briefing for an agent
#[derive(Debug, Clone)]
pub struct Briefing {
    pub agent_id: String,
    pub generated_at: DateTime<Utc>,
    pub nodes_consulted: usize,
    pub sections: Vec<BriefingSection>,
    /// Whether this was served from cache
    pub cached: bool,
}

/// One named section within a briefing
#[derive(Debug, Clone)]
pub struct BriefingSection {
    pub title: String,
    pub nodes: Vec<Node>,
}
