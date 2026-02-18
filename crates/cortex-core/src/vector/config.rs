/// Configuration for similarity thresholds and auto-linking
#[derive(Debug, Clone)]
pub struct SimilarityConfig {
    /// Minimum cosine similarity to create an auto-edge.
    /// Too low = noise, too high = misses connections.
    /// Default: 0.75
    pub auto_link_threshold: f32,

    /// Minimum similarity to flag as potential duplicate.
    /// Default: 0.92
    pub dedup_threshold: f32,

    /// Minimum similarity to flag as potential contradiction.
    /// (High similarity + opposing sentiment/content)
    /// Default: 0.80
    pub contradiction_threshold: f32,

    /// Number of nearest neighbors to check per node
    /// during auto-linking scan.
    /// Default: 20
    pub auto_link_k: usize,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            auto_link_threshold: 0.75,
            dedup_threshold: 0.92,
            contradiction_threshold: 0.80,
            auto_link_k: 20,
        }
    }
}

impl SimilarityConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the auto-linking threshold
    pub fn with_auto_link_threshold(mut self, threshold: f32) -> Self {
        self.auto_link_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the deduplication threshold
    pub fn with_dedup_threshold(mut self, threshold: f32) -> Self {
        self.dedup_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the contradiction detection threshold
    pub fn with_contradiction_threshold(mut self, threshold: f32) -> Self {
        self.contradiction_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the number of neighbors to check during auto-linking
    pub fn with_auto_link_k(mut self, k: usize) -> Self {
        self.auto_link_k = k;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.auto_link_threshold >= self.dedup_threshold {
            return Err(crate::error::CortexError::Validation(
                "auto_link_threshold must be less than dedup_threshold".to_string()
            ));
        }

        if self.contradiction_threshold >= self.dedup_threshold {
            return Err(crate::error::CortexError::Validation(
                "contradiction_threshold must be less than dedup_threshold".to_string()
            ));
        }

        if self.auto_link_k == 0 {
            return Err(crate::error::CortexError::Validation(
                "auto_link_k must be greater than 0".to_string()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SimilarityConfig::default();

        assert_eq!(config.auto_link_threshold, 0.75);
        assert_eq!(config.dedup_threshold, 0.92);
        assert_eq!(config.contradiction_threshold, 0.80);
        assert_eq!(config.auto_link_k, 20);

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_builder() {
        let config = SimilarityConfig::new()
            .with_auto_link_threshold(0.70)
            .with_dedup_threshold(0.95)
            .with_auto_link_k(30);

        assert_eq!(config.auto_link_threshold, 0.70);
        assert_eq!(config.dedup_threshold, 0.95);
        assert_eq!(config.auto_link_k, 30);
    }

    #[test]
    fn test_invalid_config() {
        let config = SimilarityConfig::new()
            .with_auto_link_threshold(0.95)
            .with_dedup_threshold(0.90); // Lower than auto_link

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_clamping() {
        let config = SimilarityConfig::new()
            .with_auto_link_threshold(1.5) // Should be clamped to 1.0
            .with_dedup_threshold(-0.5); // Should be clamped to 0.0

        assert_eq!(config.auto_link_threshold, 1.0);
        assert_eq!(config.dedup_threshold, 0.0);
    }
}
