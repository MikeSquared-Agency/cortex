use crate::error::Result;
use crate::vector::SimilarityConfig;
use std::time::Duration;

/// Configuration for the auto-linker
#[derive(Debug, Clone)]
pub struct AutoLinkerConfig {
    /// How often the linker runs. Default: 60 seconds.
    pub interval: Duration,

    /// Similarity thresholds (from Phase 3).
    pub similarity: SimilarityConfig,

    /// Run decay pass every N cycles. Default: 60 (once per hour at 60s interval).
    pub decay_every_n_cycles: u64,

    /// Run dedup scan every N cycles. Default: 360 (every 6 hours).
    pub dedup_every_n_cycles: u64,

    /// Maximum nodes to process per cycle. Prevents runaway processing
    /// if there's a bulk ingest. Default: 500.
    pub max_nodes_per_cycle: usize,

    /// Maximum edges to create per cycle. Safety valve. Default: 2000.
    pub max_edges_per_cycle: usize,

    /// Maximum auto-edges per node. Generic content prevention. Default: 50.
    pub max_edges_per_node: usize,

    /// Generic content detection threshold. If a node has this many neighbors
    /// above similarity threshold, it's flagged as too generic. Default: 30.
    pub generic_content_threshold: usize,

    /// Whether to run on startup (process backlog). Default: true.
    pub run_on_startup: bool,

    /// Decay configuration.
    pub decay: DecayConfig,
}

impl Default for AutoLinkerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            similarity: SimilarityConfig::default(),
            decay_every_n_cycles: 60,
            dedup_every_n_cycles: 360,
            max_nodes_per_cycle: 500,
            max_edges_per_cycle: 2000,
            max_edges_per_node: 50,
            generic_content_threshold: 30,
            run_on_startup: true,
            decay: DecayConfig::default(),
        }
    }
}

impl AutoLinkerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn with_similarity(mut self, config: SimilarityConfig) -> Self {
        self.similarity = config;
        self
    }

    pub fn with_decay(mut self, decay: DecayConfig) -> Self {
        self.decay = decay;
        self
    }

    pub fn with_max_nodes_per_cycle(mut self, max: usize) -> Self {
        self.max_nodes_per_cycle = max;
        self
    }

    pub fn with_max_edges_per_cycle(mut self, max: usize) -> Self {
        self.max_edges_per_cycle = max;
        self
    }

    pub fn validate(&self) -> Result<()> {
        self.similarity.validate()?;
        self.decay.validate()?;

        if self.max_nodes_per_cycle == 0 {
            return Err(crate::error::CortexError::Validation(
                "max_nodes_per_cycle must be > 0".into(),
            ));
        }

        if self.max_edges_per_cycle == 0 {
            return Err(crate::error::CortexError::Validation(
                "max_edges_per_cycle must be > 0".into(),
            ));
        }

        if self.max_edges_per_node == 0 {
            return Err(crate::error::CortexError::Validation(
                "max_edges_per_node must be > 0".into(),
            ));
        }

        Ok(())
    }
}

/// Configuration for edge decay
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Base decay rate per day. Default: 0.01 (1% per day).
    pub daily_decay_rate: f32,

    /// Minimum weight before an edge is pruned. Default: 0.1.
    pub prune_threshold: f32,

    /// Edges below this weight are candidates for deletion. Default: 0.05.
    pub delete_threshold: f32,

    /// Importance multiplier: high-importance nodes decay slower.
    /// effective_decay = daily_decay_rate × (1.0 - node.importance × importance_shield)
    /// Default: 0.8 (importance=1.0 node decays at 20% normal rate)
    pub importance_shield: f32,

    /// Access reinforcement: each access resets decay timer partially.
    /// Default: adds 7 days of "freshness"
    pub access_reinforcement_days: f32,

    /// Manual edges (human-created) are exempt from decay.
    pub exempt_manual: bool, // Default: true
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            daily_decay_rate: 0.01,
            prune_threshold: 0.1,
            delete_threshold: 0.05,
            importance_shield: 0.8,
            access_reinforcement_days: 7.0,
            exempt_manual: true,
        }
    }
}

impl DecayConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_daily_decay_rate(mut self, rate: f32) -> Self {
        self.daily_decay_rate = rate;
        self
    }

    pub fn with_prune_threshold(mut self, threshold: f32) -> Self {
        self.prune_threshold = threshold;
        self
    }

    pub fn with_delete_threshold(mut self, threshold: f32) -> Self {
        self.delete_threshold = threshold;
        self
    }

    pub fn with_importance_shield(mut self, shield: f32) -> Self {
        self.importance_shield = shield;
        self
    }

    pub fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.daily_decay_rate) {
            return Err(crate::error::CortexError::Validation(
                "daily_decay_rate must be between 0.0 and 1.0".into(),
            ));
        }

        if self.delete_threshold > self.prune_threshold {
            return Err(crate::error::CortexError::Validation(
                "delete_threshold must be <= prune_threshold".into(),
            ));
        }

        if !(0.0..=1.0).contains(&self.importance_shield) {
            return Err(crate::error::CortexError::Validation(
                "importance_shield must be between 0.0 and 1.0".into(),
            ));
        }

        Ok(())
    }
}
