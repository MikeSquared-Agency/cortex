pub mod cache;
pub mod engine;
pub mod renderer;
pub mod ingest;

pub use engine::{BriefingConfig, BriefingEngine};

use chrono::{DateTime, Utc};

use crate::types::Node;

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
