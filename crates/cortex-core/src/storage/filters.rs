use crate::types::{NodeKind, Relation};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Filter criteria for querying nodes
#[derive(Debug, Clone, Default)]
pub struct NodeFilter {
    pub kinds: Option<Vec<NodeKind>>,
    pub tags: Option<Vec<String>>,
    pub source_agent: Option<String>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub min_importance: Option<f32>,
    pub include_deleted: bool,
    /// Only return soft-deleted nodes
    pub deleted_only: bool,
    /// Only return nodes with updated_at before this time (useful for purge)
    pub updated_before: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl NodeFilter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by node kinds
    pub fn with_kinds(mut self, kinds: Vec<NodeKind>) -> Self {
        self.kinds = Some(kinds);
        self
    }

    /// Filter by tags (nodes must have at least one of these tags)
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Filter by source agent
    pub fn with_source_agent(mut self, agent: String) -> Self {
        self.source_agent = Some(agent);
        self
    }

    /// Filter by creation time (after this time)
    pub fn created_after(mut self, time: DateTime<Utc>) -> Self {
        self.created_after = Some(time);
        self
    }

    /// Filter by creation time (before this time)
    pub fn created_before(mut self, time: DateTime<Utc>) -> Self {
        self.created_before = Some(time);
        self
    }

    /// Filter by minimum importance
    pub fn with_min_importance(mut self, importance: f32) -> Self {
        self.min_importance = Some(importance);
        self
    }

    /// Include deleted nodes in results
    pub fn include_deleted(mut self) -> Self {
        self.include_deleted = true;
        self
    }

    /// Limit number of results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Skip first N results
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Only return soft-deleted nodes
    pub fn deleted_only(mut self) -> Self {
        self.deleted_only = true;
        self.include_deleted = true; // must include deleted to filter them
        self
    }

    /// Filter by updated_at (before this time)
    pub fn updated_before(mut self, time: DateTime<Utc>) -> Self {
        self.updated_before = Some(time);
        self
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub node_count: u64,
    pub edge_count: u64,
    pub node_counts_by_kind: HashMap<NodeKind, u64>,
    pub edge_counts_by_relation: HashMap<Relation, u64>,
    pub db_size_bytes: u64,
    pub oldest_node: Option<DateTime<Utc>>,
    pub newest_node: Option<DateTime<Utc>>,
}
