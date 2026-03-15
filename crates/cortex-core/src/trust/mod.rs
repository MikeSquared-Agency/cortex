//! Trust scoring engine — computes trust from graph topology, not writer assertion.
//!
//! Trust is a function of the graph state at query time. Five signals are combined:
//! corroboration, contradiction penalty, source reliability, access reinforcement, freshness.

pub mod cache;
pub mod signals;

use crate::error::Result;
use crate::storage::Storage;
use crate::types::Node;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Configuration for the trust scoring engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustConfig {
    /// Number of independent corroborations for max score.
    pub corroboration_saturation: u32,
    /// Minimum edge weight to count as corroboration.
    pub corroboration_min_weight: f32,
    /// Penalty per contradiction edge (floored at 0.0).
    pub contradiction_weight: f32,
    /// Number of accesses for max access signal.
    pub access_saturation: u64,
    /// Days until freshness signal reaches 0.0.
    pub freshness_halflife: f64,
    /// Signal combination weights.
    pub weights: TrustWeights,
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            corroboration_saturation: 3,
            corroboration_min_weight: 0.6,
            contradiction_weight: 0.3,
            access_saturation: 20,
            freshness_halflife: 90.0,
            weights: TrustWeights::default(),
        }
    }
}

/// Weights for combining the five trust signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustWeights {
    pub corroboration: f32,
    pub contradiction: f32,
    pub source: f32,
    pub access: f32,
    pub freshness: f32,
}

impl Default for TrustWeights {
    fn default() -> Self {
        Self {
            corroboration: 0.30,
            contradiction: 0.25,
            source: 0.20,
            access: 0.15,
            freshness: 0.10,
        }
    }
}

/// Detailed trust breakdown for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    /// Combined score in [0.0, 1.0].
    pub total: f32,
    /// Corroboration signal value.
    pub corroboration: f32,
    /// Contradiction signal value (1.0 = no contradictions).
    pub contradiction: f32,
    /// Source reliability of the creating agent.
    pub source_reliability: f32,
    /// Access reinforcement signal.
    pub access: f32,
    /// Freshness signal.
    pub freshness: f32,
    /// Which agents corroborated this node.
    pub corroborating_agents: Vec<String>,
    /// Number of contradiction edges.
    pub contradiction_count: u32,
}

/// Trust scoring engine. Computes trust from graph topology at query time.
pub struct TrustEngine<S: Storage> {
    storage: Arc<S>,
    config: TrustConfig,
    source_cache: Mutex<HashMap<String, f32>>,
}

impl<S: Storage> TrustEngine<S> {
    pub fn new(storage: Arc<S>, config: TrustConfig) -> Self {
        Self {
            storage,
            config,
            source_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Compute trust score for a single node.
    pub fn score(&self, node: &Node) -> Result<TrustScore> {
        let (corroboration_val, corroborating_agents) =
            signals::corroboration(self.storage.as_ref(), node, &self.config)?;
        let (contradiction_val, contradiction_count) =
            signals::contradiction(self.storage.as_ref(), node, &self.config)?;
        let source_val = self.source_reliability(&node.source.agent)?;
        let access_val = signals::access_reinforcement(node, &self.config);
        let freshness_val = signals::freshness(node, &self.config);

        let w = &self.config.weights;
        let total = corroboration_val * w.corroboration
            + contradiction_val * w.contradiction
            + source_val * w.source
            + access_val * w.access
            + freshness_val * w.freshness;

        Ok(TrustScore {
            total: total.clamp(0.0, 1.0),
            corroboration: corroboration_val,
            contradiction: contradiction_val,
            source_reliability: source_val,
            access: access_val,
            freshness: freshness_val,
            corroborating_agents,
            contradiction_count,
        })
    }

    /// Compute trust scores for a batch of nodes.
    /// More efficient: amortises source reliability lookups.
    pub fn score_batch(&self, nodes: &[Node]) -> Result<Vec<TrustScore>> {
        // Pre-warm source cache for all unique agents in this batch.
        let unique_agents: Vec<String> = {
            let mut agents: Vec<String> = nodes.iter().map(|n| n.source.agent.clone()).collect();
            agents.sort();
            agents.dedup();
            agents
        };
        for agent in &unique_agents {
            let _ = self.source_reliability(agent)?;
        }

        nodes.iter().map(|n| self.score(n)).collect()
    }

    /// Refresh the source reliability cache by recomputing all agents.
    pub fn refresh_source_cache(&self) -> Result<()> {
        let agents: Vec<String> = {
            let cache = self.source_cache.lock().unwrap();
            cache.keys().cloned().collect()
        };
        let mut new_cache = HashMap::new();
        for agent in &agents {
            let reliability = cache::compute_source_reliability(self.storage.as_ref(), agent)?;
            new_cache.insert(agent.clone(), reliability);
        }
        let mut cache = self.source_cache.lock().unwrap();
        *cache = new_cache;
        Ok(())
    }

    /// Get agent reliability scores for all cached agents.
    pub fn agent_reliabilities(&self) -> HashMap<String, f32> {
        self.source_cache.lock().unwrap().clone()
    }

    /// Get or compute source reliability for a given agent.
    fn source_reliability(&self, agent_id: &str) -> Result<f32> {
        // Check cache first.
        {
            let cache = self.source_cache.lock().unwrap();
            if let Some(&val) = cache.get(agent_id) {
                return Ok(val);
            }
        }

        let reliability = cache::compute_source_reliability(self.storage.as_ref(), agent_id)?;

        let mut cache = self.source_cache.lock().unwrap();
        cache.insert(agent_id.to_string(), reliability);
        Ok(reliability)
    }

    /// Access the config (useful for HTTP serialisation).
    pub fn config(&self) -> &TrustConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{NodeKind, Source};
    use tempfile::TempDir;

    fn test_storage() -> (Arc<RedbStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = RedbStorage::open(dir.path().join("test.redb")).unwrap();
        (Arc::new(storage), dir)
    }

    /// Helper: create a test node with given agent and access patterns.
    fn test_node(agent: &str, importance: f32, access_count: u64) -> Node {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Test fact".to_string(),
            "Body".to_string(),
            Source {
                agent: agent.to_string(),
                session: None,
                channel: None,
            },
            importance,
        );
        node.access_count = access_count;
        node
    }

    #[test]
    fn test_default_config_weights_sum_to_one() {
        let w = TrustWeights::default();
        let sum = w.corroboration + w.contradiction + w.source + w.access + w.freshness;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Weights should sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn test_trust_score_zero_edges_baseline() {
        let (storage, _dir) = test_storage();

        let node = test_node("agent-a", 0.5, 5);
        storage.put_node(&node).unwrap();

        let engine = TrustEngine::new(storage, TrustConfig::default());
        let score = engine.score(&node).unwrap();

        assert_eq!(score.corroboration, 0.0);
        assert_eq!(score.contradiction, 1.0);
        assert!(score.total > 0.0, "Baseline trust should be > 0");
        assert!(score.total <= 1.0);
    }

    #[test]
    fn test_trust_score_deterministic() {
        let (storage, _dir) = test_storage();
        let node = test_node("agent-a", 0.5, 10);
        storage.put_node(&node).unwrap();

        let engine = TrustEngine::new(storage, TrustConfig::default());
        let s1 = engine.score(&node).unwrap();
        let s2 = engine.score(&node).unwrap();
        assert_eq!(
            s1.total, s2.total,
            "Same graph state should produce same scores"
        );
    }

    #[test]
    fn test_batch_scoring() {
        let (storage, _dir) = test_storage();
        let nodes: Vec<Node> = (0..5)
            .map(|i| {
                let n = test_node(&format!("agent-{i}"), 0.5, i as u64);
                storage.put_node(&n).unwrap();
                n
            })
            .collect();

        let engine = TrustEngine::new(storage, TrustConfig::default());
        let scores = engine.score_batch(&nodes).unwrap();
        assert_eq!(scores.len(), 5);

        // Individual scores should match batch scores.
        for (node, batch_score) in nodes.iter().zip(scores.iter()) {
            let individual = engine.score(node).unwrap();
            assert!(
                (individual.total - batch_score.total).abs() < 0.001,
                "Batch and individual scores should match"
            );
        }
    }
}
