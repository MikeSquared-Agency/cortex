use crate::types::Node;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for query-time score decay.
///
/// Controls how temporal freshness and usage frequency influence search
/// result ranking. Decay affects *ranking*, not *existence* — nodes are
/// never deleted by score decay.
///
/// # Formula
/// ```text
/// temporal_factor = max(min_factor, exp(-kind_rate × days_idle))
/// echo_factor     = min(echo_cap, 1.0 + access_count × echo_weight)
/// final_score     = raw × (1 - recency_w)
///                 + raw × temporal_factor × echo_factor × recency_w
/// ```
///
/// When `recency_weight = 0`, this reduces to `raw_score` (fully backward
/// compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreDecayConfig {
    /// Enable query-time score decay. When false, `apply_score_decay` is a no-op.
    pub enabled: bool,

    /// Default daily decay rate (e.g. `0.02` = 2% per day).
    /// Overridden per kind via `by_kind`.
    pub daily_rate: f64,

    /// Days beyond which `temporal_factor` floors at `min_factor`.
    pub max_age_days: f64,

    /// Minimum temporal factor. Old nodes never fully disappear from results.
    pub min_factor: f64,

    /// Per-access echo boost weight. Each retrieval adds `echo_weight` to the
    /// multiplier (before capping at `echo_cap`).
    pub echo_weight: f64,

    /// Maximum echo multiplier. A value of `2.0` means heavily-used nodes
    /// score at most 2× a never-used node, all else equal.
    pub echo_cap: f64,

    /// Default blend weight for the recency component in the final score.
    /// `0.0` = pure relevance; `1.0` = recency dominates.
    /// Overridden per query via the `recency_bias` query parameter.
    pub recency_weight: f32,

    /// Per-kind daily decay rate overrides (key = node kind string).
    /// Events decay faster; decisions and patterns stay relevant longer.
    pub by_kind: HashMap<String, f64>,
}

impl Default for ScoreDecayConfig {
    fn default() -> Self {
        let mut by_kind = HashMap::new();
        by_kind.insert("event".to_string(), 0.05);
        by_kind.insert("observation".to_string(), 0.04);
        by_kind.insert("decision".to_string(), 0.005);
        by_kind.insert("pattern".to_string(), 0.005);
        by_kind.insert("fact".to_string(), 0.01);
        by_kind.insert("preference".to_string(), 0.005);
        Self {
            enabled: true,
            daily_rate: 0.02,
            max_age_days: 365.0,
            min_factor: 0.1,
            echo_weight: 0.05,
            echo_cap: 2.0,
            recency_weight: 0.15,
            by_kind,
        }
    }
}

/// Apply query-time score decay to a raw similarity score.
///
/// `recency_bias` overrides `config.recency_weight` for this query.
/// Pass `config.recency_weight` as `recency_bias` to use the configured default.
///
/// Returns `raw_score` unchanged when `!config.enabled` or `recency_bias == 0.0`.
pub fn apply_score_decay(
    node: &Node,
    raw_score: f32,
    config: &ScoreDecayConfig,
    recency_bias: f32,
) -> f32 {
    if !config.enabled || recency_bias == 0.0 {
        return raw_score;
    }

    let now = Utc::now();
    let days_idle = now
        .signed_duration_since(node.last_accessed_at)
        .num_seconds()
        .max(0) as f64
        / 86_400.0;

    let kind_rate = config
        .by_kind
        .get(node.kind.as_str())
        .copied()
        .unwrap_or(config.daily_rate);

    let effective_days = days_idle.min(config.max_age_days);
    let temporal_factor = (-kind_rate * effective_days)
        .exp()
        .max(config.min_factor) as f32;

    let echo_factor = (1.0 + node.access_count as f64 * config.echo_weight)
        .min(config.echo_cap) as f32;

    raw_score * (1.0 - recency_bias) + raw_score * temporal_factor * echo_factor * recency_bias
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Node, NodeKind, Source};
    use chrono::Duration;

    fn make_node(kind: &str) -> Node {
        Node::new(
            NodeKind::new(kind).unwrap(),
            "Test".to_string(),
            "Test body".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    #[test]
    fn test_decay_disabled_returns_raw() {
        let node = make_node("fact");
        let config = ScoreDecayConfig {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(apply_score_decay(&node, 0.8, &config, 0.15), 0.8);
    }

    #[test]
    fn test_zero_recency_bias_returns_raw() {
        let node = make_node("fact");
        let config = ScoreDecayConfig::default();
        assert_eq!(apply_score_decay(&node, 0.8, &config, 0.0), 0.8);
    }

    #[test]
    fn test_fresh_node_no_decay() {
        // Node created just now — last_accessed_at = now = 0 days idle
        let node = make_node("fact");
        let config = ScoreDecayConfig::default();
        // temporal = exp(0) = 1.0, echo = 1.0 (access_count = 0)
        // final = 0.8 * 0.85 + 0.8 * 1.0 * 1.0 * 0.15 = 0.68 + 0.12 = 0.80
        let result = apply_score_decay(&node, 0.8, &config, config.recency_weight);
        assert!((result - 0.8).abs() < 0.01, "fresh node should score ~0.8, got {}", result);
    }

    #[test]
    fn test_stale_node_decays() {
        let mut node = make_node("fact");
        // Simulate a node not accessed for 100 days
        node.last_accessed_at = Utc::now() - Duration::days(100);

        let config = ScoreDecayConfig::default();
        // kind_rate = 0.01 (fact), days = 100
        // temporal = exp(-0.01 * 100) = exp(-1) ≈ 0.368
        let result = apply_score_decay(&node, 0.8, &config, config.recency_weight);
        let no_decay = 0.8;
        assert!(result < no_decay, "stale node should score less than raw: {} < {}", result, no_decay);
    }

    #[test]
    fn test_floor_at_min_factor() {
        let mut node = make_node("event");
        // Simulate 400 days idle (past max_age_days = 365)
        node.last_accessed_at = Utc::now() - Duration::days(400);

        let config = ScoreDecayConfig::default();
        // temporal = max(0.1, exp(-0.05 * 365)) = 0.1 (floored)
        // echo = 1.0 (access_count = 0)
        // final = 0.8 * 0.85 + 0.8 * 0.1 * 1.0 * 0.15 = 0.68 + 0.012 = 0.692
        let result = apply_score_decay(&node, 0.8, &config, config.recency_weight);
        // The floor should prevent going below: 0.8 * 0.85 + 0.8 * 0.1 * 1.0 * 0.15
        let floor_score = 0.8 * (1.0 - config.recency_weight)
            + 0.8 * config.min_factor as f32 * 1.0 * config.recency_weight;
        assert!(
            (result - floor_score).abs() < 0.01,
            "expected floor score ~{}, got {}",
            floor_score,
            result
        );
    }

    #[test]
    fn test_echo_boost_capped() {
        let mut node = make_node("fact");
        node.access_count = 10_000; // Very high access count

        let config = ScoreDecayConfig::default();
        // echo = min(2.0, 1.0 + 10000 * 0.05) = min(2.0, 501) = 2.0
        // temporal = 1.0 (fresh node)
        // final = 0.8 * 0.85 + 0.8 * 1.0 * 2.0 * 0.15 = 0.68 + 0.24 = 0.92
        let result = apply_score_decay(&node, 0.8, &config, config.recency_weight);
        let expected = 0.8 * (1.0 - config.recency_weight)
            + 0.8 * 1.0 * config.echo_cap as f32 * config.recency_weight;
        assert!(
            (result - expected).abs() < 0.01,
            "expected echo-capped score ~{}, got {}",
            expected,
            result
        );
    }

    #[test]
    fn test_kind_rate_override() {
        let mut decision = make_node("decision");
        let mut event = make_node("event");

        // Both idle for 30 days
        decision.last_accessed_at = Utc::now() - Duration::days(30);
        event.last_accessed_at = Utc::now() - Duration::days(30);

        let config = ScoreDecayConfig::default();
        // decision rate = 0.005, event rate = 0.05
        // decision temporal = exp(-0.005 * 30) = exp(-0.15) ≈ 0.861
        // event temporal    = exp(-0.05  * 30) = exp(-1.5)  ≈ 0.223
        let decision_score = apply_score_decay(&decision, 0.8, &config, config.recency_weight);
        let event_score = apply_score_decay(&event, 0.8, &config, config.recency_weight);
        assert!(
            decision_score > event_score,
            "decisions should decay slower than events: {} > {}",
            decision_score,
            event_score
        );
    }

    #[test]
    fn test_recency_bias_zero_equals_raw() {
        let mut node = make_node("fact");
        node.last_accessed_at = Utc::now() - Duration::days(200);
        node.access_count = 5;

        let config = ScoreDecayConfig::default();
        let result = apply_score_decay(&node, 0.75, &config, 0.0);
        assert_eq!(result, 0.75, "recency_bias=0 must return raw_score");
    }

    #[test]
    fn test_recency_bias_one_full_decay() {
        let node = make_node("fact");
        let config = ScoreDecayConfig::default();
        // recency_bias = 1.0: final = 0 + raw * temporal * echo * 1.0
        // For a fresh node: temporal = 1.0, echo = 1.0 → final = raw
        let result = apply_score_decay(&node, 0.9, &config, 1.0);
        assert!(
            (result - 0.9).abs() < 0.01,
            "recency_bias=1 with fresh node should ≈ raw: {}",
            result
        );
    }
}
