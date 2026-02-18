use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Metrics for auto-linker observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoLinkerMetrics {
    /// Total cycles completed.
    pub cycles: u64,

    /// Nodes processed this cycle.
    pub nodes_processed: u64,

    /// Edges created this cycle.
    pub edges_created: u64,

    /// Edges pruned by decay this cycle.
    pub edges_pruned: u64,

    /// Edges deleted this cycle.
    pub edges_deleted: u64,

    /// Duplicates detected this cycle.
    pub duplicates_found: u64,

    /// Contradictions flagged this cycle.
    pub contradictions_found: u64,

    /// Processing time for last cycle.
    #[serde(with = "duration_serializer")]
    pub last_cycle_duration: Duration,

    /// Current cursor position.
    pub cursor: DateTime<Utc>,

    /// Backlog: nodes awaiting processing.
    pub backlog_size: u64,

    /// Total nodes in graph.
    pub total_nodes: u64,

    /// Total edges in graph.
    pub total_edges: u64,
}

impl Default for AutoLinkerMetrics {
    fn default() -> Self {
        Self {
            cycles: 0,
            nodes_processed: 0,
            edges_created: 0,
            edges_pruned: 0,
            edges_deleted: 0,
            duplicates_found: 0,
            contradictions_found: 0,
            last_cycle_duration: Duration::from_secs(0),
            cursor: Utc::now(),
            backlog_size: 0,
            total_nodes: 0,
            total_edges: 0,
        }
    }
}

impl AutoLinkerMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset per-cycle metrics (called at start of each cycle)
    pub fn reset_cycle_metrics(&mut self) {
        self.nodes_processed = 0;
        self.edges_created = 0;
        self.edges_pruned = 0;
        self.edges_deleted = 0;
        self.duplicates_found = 0;
        self.contradictions_found = 0;
    }

    /// Increment cycle counter
    pub fn increment_cycle(&mut self) {
        self.cycles += 1;
    }

    /// Update cursor position
    pub fn update_cursor(&mut self, cursor: DateTime<Utc>) {
        self.cursor = cursor;
    }

    /// Set cycle duration
    pub fn set_cycle_duration(&mut self, duration: Duration) {
        self.last_cycle_duration = duration;
    }

    /// Add nodes processed
    pub fn add_nodes_processed(&mut self, count: u64) {
        self.nodes_processed += count;
    }

    /// Add edges created
    pub fn add_edges_created(&mut self, count: u64) {
        self.edges_created += count;
    }

    /// Add edges pruned
    pub fn add_edges_pruned(&mut self, count: u64) {
        self.edges_pruned += count;
    }

    /// Add edges deleted
    pub fn add_edges_deleted(&mut self, count: u64) {
        self.edges_deleted += count;
    }

    /// Add duplicates found
    pub fn add_duplicates_found(&mut self, count: u64) {
        self.duplicates_found += count;
    }

    /// Add contradictions found
    pub fn add_contradictions_found(&mut self, count: u64) {
        self.contradictions_found += count;
    }

    /// Update backlog size
    pub fn set_backlog_size(&mut self, size: u64) {
        self.backlog_size = size;
    }

    /// Update total nodes
    pub fn set_total_nodes(&mut self, count: u64) {
        self.total_nodes = count;
    }

    /// Update total edges
    pub fn set_total_edges(&mut self, count: u64) {
        self.total_edges = count;
    }

    /// Get a summary string for logging
    pub fn summary(&self) -> String {
        format!(
            "Cycle #{}: processed {} nodes, created {} edges, pruned {}, deleted {}, \
             found {} duplicates, {} contradictions in {:?} | Backlog: {} | Total: {} nodes, {} edges",
            self.cycles,
            self.nodes_processed,
            self.edges_created,
            self.edges_pruned,
            self.edges_deleted,
            self.duplicates_found,
            self.contradictions_found,
            self.last_cycle_duration,
            self.backlog_size,
            self.total_nodes,
            self.total_edges
        )
    }
}

// Custom serializer for Duration
mod duration_serializer {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}
