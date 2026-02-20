use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Blend weight between historical edge weight and context fit (50/50).
const BLEND: f32 = 0.5;

/// Context signals extracted from the current conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSignals {
    /// User sentiment: 0.0 = very frustrated, 1.0 = very pleased
    #[serde(default = "default_half")]
    pub sentiment: f32,

    /// Detected task type: coding | planning | casual | crisis | reflection
    #[serde(default = "default_casual")]
    pub task_type: String,

    /// Rolling rate of user corrections in this session (0.0–1.0)
    #[serde(default)]
    pub correction_rate: f32,

    /// Semantic distance from conversation start (0.0 = same topic, 1.0 = very different)
    #[serde(default)]
    pub topic_shift: f32,

    /// User energy proxy: 0.0 = low, 1.0 = high
    #[serde(default = "default_half")]
    pub energy: f32,
}

fn default_half() -> f32 {
    0.5
}
fn default_casual() -> String {
    "casual".to_string()
}

impl Default for ContextSignals {
    fn default() -> Self {
        Self {
            sentiment: 0.5,
            task_type: "casual".to_string(),
            correction_rate: 0.0,
            topic_shift: 0.0,
            energy: 0.5,
        }
    }
}

impl ContextSignals {
    /// Look up a named signal value without allocating a map.
    ///
    /// Recognised keys:
    /// - `sentiment_high` / `user_pleased` — current sentiment (alias pair; prefer `user_pleased`)
    /// - `user_frustrated` — 1.0 − sentiment
    /// - `correction_rate_high` — correction_rate
    /// - `topic_shift_high` — topic_shift
    /// - `energy_high` — energy
    /// - `task_<type>` — 1.0 if `task_type == <type>` (case-insensitive), 0.0 otherwise
    ///
    /// Unknown keys return 0.0.
    pub fn get_signal(&self, key: &str) -> f32 {
        match key {
            "sentiment_high" | "user_pleased" => self.sentiment,
            "user_frustrated" => 1.0 - self.sentiment,
            "correction_rate_high" => self.correction_rate,
            "topic_shift_high" => self.topic_shift,
            "energy_high" => self.energy,
            _ => {
                // task_<type> one-hot: 1.0 for the active type, 0.0 for all others
                if let Some(task_name) = key.strip_prefix("task_") {
                    if task_name.eq_ignore_ascii_case(&self.task_type) {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            }
        }
    }

    /// Build a named signal map for external callers that need the full vector.
    ///
    /// Prefer [`get_signal`] when iterating `context_weights` keys — it avoids allocation.
    pub fn to_signal_map(&self) -> HashMap<String, f32> {
        let mut m = HashMap::with_capacity(12);
        // `user_pleased` is canonical; `sentiment_high` is kept as a backwards-compat alias
        m.insert("user_pleased".into(), self.sentiment);
        m.insert("sentiment_high".into(), self.sentiment);
        m.insert("user_frustrated".into(), 1.0 - self.sentiment);
        m.insert("correction_rate_high".into(), self.correction_rate);
        m.insert("topic_shift_high".into(), self.topic_shift);
        m.insert("energy_high".into(), self.energy);
        // Task-type one-hot
        for tt in ["coding", "planning", "casual", "crisis", "reflection"] {
            m.insert(format!("task_{tt}"), if tt.eq_ignore_ascii_case(&self.task_type) { 1.0 } else { 0.0 });
        }
        m
    }
}

/// Compute the normalised context fit for a prompt variant.
///
/// Iterates the variant's `context_weights` map and performs a weighted dot product
/// against the current context signals, normalised by the sum of absolute weights.
///
/// Returns `None` when there are no usable context weights (absent, empty, or all-zero).
/// Returns a value clamped to \[0.0, 1.0\] otherwise.
///
/// Negative weights are supported: they penalise signals that are active.
///
/// **Note on all-negative weights:** When every weight is negative and signals are
/// active, the dot product will be negative, clamping the result to 0.0. To get
/// meaningful differentiation with anti-pattern weights, pair them with at least one
/// positive weight (e.g. the desired signal) so the score can range above zero.
pub fn context_fit(
    context_weights: Option<&serde_json::Value>,
    signals: &ContextSignals,
) -> Option<f32> {
    let cw = context_weights?.as_object().filter(|m| !m.is_empty())?;

    let mut dot = 0.0f32;
    let mut abs_weight_sum = 0.0f32;

    for (key, wv) in cw {
        let w = wv.as_f64().unwrap_or(0.0) as f32;
        let s = signals.get_signal(key);
        dot += s * w;
        abs_weight_sum += w.abs();
    }

    if abs_weight_sum < f32::EPSILON {
        return None;
    }

    Some((dot / abs_weight_sum).clamp(0.0, 1.0))
}

/// Score a prompt variant against current context signals.
///
/// Returns a value in \[0.0, 1.0\]:
/// - `BLEND` (50%) from `edge_weight` — historical performance
/// - `1 - BLEND` (50%) from context fit (dot product of signals × variant's `context_weights`)
///
/// Falls back to `edge_weight` unchanged if the variant has no usable `context_weights`.
///
/// Use [`context_fit`] directly if you need the two components separately.
pub fn score_variant(
    edge_weight: f32,
    context_weights: Option<&serde_json::Value>,
    signals: &ContextSignals,
) -> f32 {
    match context_fit(context_weights, signals) {
        None => edge_weight,
        Some(fit) => (BLEND * edge_weight + (1.0 - BLEND) * fit).clamp(0.0, 1.0),
    }
}

/// Compute an observation score from interaction outcomes.
///
/// - `sentiment`: 0.0–1.0
/// - `correction_count`: number of corrections the user made
/// - `task_outcome`: "success" | "partial" | "failure" | "unknown"
///
/// Returns a value in \[0.0, 1.0\].
pub fn observation_score(sentiment: f32, correction_count: u32, task_outcome: &str) -> f32 {
    let task_success: f32 = match task_outcome {
        "success" => 1.0,
        "partial" => 0.5,
        _ => 0.0, // "failure" | "unknown"
    };
    // correction_penalty saturates at 1.0 after 10 corrections
    let correction_penalty = (correction_count as f32 * 0.1).min(1.0);
    (0.5 * sentiment + 0.3 * (1.0 - correction_penalty) + 0.2 * task_success).clamp(0.0, 1.0)
}

/// Update an edge weight using exponential moving average (α = 0.1).
///
/// Slow adaptation (α = 0.1) avoids thrashing on a single bad interaction.
/// After ~22 observations from a neutral start (0.5) with perfect scores,
/// the weight converges to ~0.9.
pub fn update_edge_weight(old_weight: f32, obs_score: f32) -> f32 {
    const ALPHA: f32 = 0.1;
    (ALPHA.mul_add(obs_score - old_weight, old_weight)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── get_signal ────────────────────────────────────────────────────────────

    #[test]
    fn get_signal_sentiment_keys() {
        let s = ContextSignals { sentiment: 0.8, ..Default::default() };
        assert!((s.get_signal("user_pleased") - 0.8).abs() < f32::EPSILON);
        assert!((s.get_signal("sentiment_high") - 0.8).abs() < f32::EPSILON);
        assert!((s.get_signal("user_frustrated") - 0.2).abs() < 1e-6);
    }

    #[test]
    fn get_signal_continuous_keys() {
        let s = ContextSignals {
            correction_rate: 0.3,
            topic_shift: 0.7,
            energy: 0.6,
            ..Default::default()
        };
        assert!((s.get_signal("correction_rate_high") - 0.3).abs() < f32::EPSILON);
        assert!((s.get_signal("topic_shift_high") - 0.7).abs() < f32::EPSILON);
        assert!((s.get_signal("energy_high") - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn get_signal_task_type_active_is_one() {
        let s = ContextSignals { task_type: "coding".into(), ..Default::default() };
        assert!((s.get_signal("task_coding") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn get_signal_task_type_inactive_is_zero() {
        let s = ContextSignals { task_type: "coding".into(), ..Default::default() };
        for inactive in ["task_planning", "task_casual", "task_crisis", "task_reflection"] {
            assert_eq!(s.get_signal(inactive), 0.0, "expected 0 for {inactive}");
        }
    }

    #[test]
    fn get_signal_task_type_case_insensitive() {
        // Clients may send "CODING" or "Coding" — should still match
        let s = ContextSignals { task_type: "CODING".into(), ..Default::default() };
        assert!((s.get_signal("task_coding") - 1.0).abs() < f32::EPSILON);
        let s2 = ContextSignals { task_type: "coding".into(), ..Default::default() };
        assert!((s2.get_signal("task_CODING") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn get_signal_unknown_key_returns_zero() {
        let s = ContextSignals::default();
        assert_eq!(s.get_signal("nonexistent_key"), 0.0);
        assert_eq!(s.get_signal(""), 0.0);
    }

    #[test]
    fn get_signal_unknown_task_prefix_returns_zero() {
        let s = ContextSignals { task_type: "coding".into(), ..Default::default() };
        // "task_debugging" is not a known type and doesn't match task_type
        assert_eq!(s.get_signal("task_debugging"), 0.0);
    }

    // ── context_fit ───────────────────────────────────────────────────────────

    #[test]
    fn context_fit_none_for_null() {
        assert!(context_fit(None, &ContextSignals::default()).is_none());
    }

    #[test]
    fn context_fit_none_for_empty_object() {
        let cw = serde_json::json!({});
        assert!(context_fit(Some(&cw), &ContextSignals::default()).is_none());
    }

    #[test]
    fn context_fit_none_for_all_zero_weights() {
        let cw = serde_json::json!({ "user_pleased": 0.0, "task_coding": 0.0 });
        assert!(context_fit(Some(&cw), &ContextSignals::default()).is_none());
    }

    #[test]
    fn context_fit_correct_value() {
        let signals = ContextSignals {
            sentiment: 0.1, // user_frustrated = 0.9
            task_type: "coding".into(),
            ..Default::default()
        };
        let cw = serde_json::json!({
            "user_frustrated": 0.9,
            "task_coding": 0.3,
        });
        // dot  = 0.9 * 0.9 + 0.3 * 1.0 = 0.81 + 0.30 = 1.11
        // abs_sum = 0.9 + 0.3 = 1.2
        // fit = 1.11 / 1.2 = 0.925
        let fit = context_fit(Some(&cw), &signals).unwrap();
        assert!((fit - 0.925).abs() < 0.001, "got {fit}");
    }

    #[test]
    fn context_fit_negative_weight_penalises() {
        // Variant that dislikes high-energy users: "energy_high": -0.8
        let cw = serde_json::json!({ "energy_high": -0.8 });
        let high_energy = ContextSignals { energy: 1.0, ..Default::default() };
        let low_energy = ContextSignals { energy: 0.0, ..Default::default() };

        // High energy: dot = -0.8 * 1.0 = -0.8 / abs_sum 0.8 = -1.0 → clamped to 0.0
        let fit_high = context_fit(Some(&cw), &high_energy).unwrap();
        assert!(fit_high < 0.01, "high-energy should be penalised, got {fit_high}");

        // Low energy: dot = -0.8 * 0.0 = 0 / 0.8 = 0.0
        let fit_low = context_fit(Some(&cw), &low_energy).unwrap();
        assert!(fit_low < 0.01, "low-energy neutral, got {fit_low}");
    }

    #[test]
    fn context_fit_ignores_unknown_keys() {
        let signals = ContextSignals::default();
        let cw = serde_json::json!({ "completely_unknown_signal": 1.0 });
        // dot = 0.0 * 1.0 = 0.0, abs_sum = 1.0, fit = 0.0
        let fit = context_fit(Some(&cw), &signals).unwrap();
        assert!(fit < f32::EPSILON, "got {fit}");
    }

    // ── score_variant ─────────────────────────────────────────────────────────

    #[test]
    fn score_variant_no_context_weights_returns_edge_weight() {
        let signals = ContextSignals::default();
        assert_eq!(score_variant(0.7, None, &signals), 0.7);
    }

    #[test]
    fn score_variant_empty_context_weights_returns_edge_weight() {
        let signals = ContextSignals::default();
        let cw = serde_json::json!({});
        assert_eq!(score_variant(0.6, Some(&cw), &signals), 0.6);
    }

    #[test]
    fn score_variant_consistent_with_context_fit() {
        let signals = ContextSignals {
            sentiment: 0.2,
            task_type: "crisis".into(),
            ..Default::default()
        };
        let cw = serde_json::json!({ "user_frustrated": 0.8, "task_crisis": 0.9 });
        let fit = context_fit(Some(&cw), &signals).unwrap();
        let edge_weight = 0.6;
        let expected = (BLEND * edge_weight + (1.0 - BLEND) * fit).clamp(0.0, 1.0);
        let actual = score_variant(edge_weight, Some(&cw), &signals);
        assert!((actual - expected).abs() < 1e-6, "got {actual}, expected {expected}");
    }

    #[test]
    fn score_variant_prefers_contextually_matched_variant() {
        let signals = ContextSignals {
            sentiment: 0.1, // frustrated
            task_type: "coding".into(),
            ..Default::default()
        };

        let cw_match = serde_json::json!({
            "user_frustrated": 0.9,
            "task_coding": 0.8,
        });
        let cw_mismatch = serde_json::json!({
            "user_pleased": 0.9,
            "task_casual": 0.8,
        });

        let matched = score_variant(0.5, Some(&cw_match), &signals);
        let mismatched = score_variant(0.5, Some(&cw_mismatch), &signals);
        assert!(matched > mismatched, "matched={matched:.3} should beat mismatched={mismatched:.3}");
    }

    #[test]
    fn score_variant_clamps_to_unit_interval() {
        // Edge weight = 1.0 and perfect context fit = 1.0 should stay at 1.0
        let signals = ContextSignals { energy: 1.0, ..Default::default() };
        let cw = serde_json::json!({ "energy_high": 1.0 });
        let score = score_variant(1.0, Some(&cw), &signals);
        assert!(score <= 1.0 && score >= 0.0, "out of range: {score}");
    }

    // ── observation_score ─────────────────────────────────────────────────────

    #[test]
    fn observation_score_success_case() {
        // 0.5*0.8 + 0.3*(1-0.1) + 0.2*1.0 = 0.4 + 0.27 + 0.2 = 0.87
        let s = observation_score(0.8, 1, "success");
        assert!((s - 0.87).abs() < 0.01, "got {s}");
    }

    #[test]
    fn observation_score_partial_outcome() {
        // 0.5*0.5 + 0.3*1.0 + 0.2*0.5 = 0.25 + 0.30 + 0.10 = 0.65
        let s = observation_score(0.5, 0, "partial");
        assert!((s - 0.65).abs() < 0.01, "got {s}");
    }

    #[test]
    fn observation_score_failure_case() {
        // 0.5*0.2 + 0.3*(1-0.5) + 0.2*0.0 = 0.10 + 0.15 + 0.0 = 0.25
        let s = observation_score(0.2, 5, "failure");
        assert!((s - 0.25).abs() < 0.01, "got {s}");
    }

    #[test]
    fn observation_score_unknown_outcome() {
        // Same as failure (0.0 task_success)
        let s_unknown = observation_score(0.5, 0, "unknown");
        let s_failure = observation_score(0.5, 0, "failure");
        assert!((s_unknown - s_failure).abs() < f32::EPSILON, "unknown should == failure");
    }

    #[test]
    fn observation_score_max_corrections_saturates_penalty() {
        // 10 or more corrections → penalty = 1.0, correction term = 0
        let s10 = observation_score(0.5, 10, "success");
        let s20 = observation_score(0.5, 20, "success");
        assert!((s10 - s20).abs() < f32::EPSILON, "saturation should produce identical scores");
    }

    #[test]
    fn observation_score_best_possible() {
        // sentiment=1.0, no corrections, success → should be 1.0
        // 0.5*1.0 + 0.3*1.0 + 0.2*1.0 = 1.0
        let s = observation_score(1.0, 0, "success");
        assert!((s - 1.0).abs() < f32::EPSILON, "got {s}");
    }

    #[test]
    fn observation_score_worst_possible() {
        // sentiment=0.0, 10+ corrections, failure → 0.0
        let s = observation_score(0.0, 10, "failure");
        assert!(s < f32::EPSILON, "got {s}");
    }

    // ── update_edge_weight ────────────────────────────────────────────────────

    #[test]
    fn update_edge_weight_positive_observation() {
        // 0.9*0.8 + 0.1*1.0 = 0.72 + 0.10 = 0.82
        let w = update_edge_weight(0.8, 1.0);
        assert!((w - 0.82).abs() < 0.001, "got {w}");
    }

    #[test]
    fn update_edge_weight_negative_observation() {
        // 0.9*0.8 + 0.1*0.0 = 0.72
        let w = update_edge_weight(0.8, 0.0);
        assert!((w - 0.72).abs() < 0.001, "got {w}");
    }

    #[test]
    fn update_edge_weight_clamps_to_one() {
        let w = update_edge_weight(1.0, 1.0);
        assert!(w <= 1.0, "exceeded 1.0: {w}");
    }

    #[test]
    fn update_edge_weight_clamps_to_zero() {
        let w = update_edge_weight(0.0, 0.0);
        assert!(w >= 0.0, "below 0.0: {w}");
    }

    #[test]
    fn update_edge_weight_ema_convergence() {
        // Starting at 0.5 with perfect observations, should converge toward 1.0
        let mut w = 0.5f32;
        for _ in 0..100 {
            w = update_edge_weight(w, 1.0);
        }
        assert!(w > 0.99, "failed to converge: {w}");
    }

    #[test]
    fn update_edge_weight_ema_convergence_toward_score() {
        // Starting at 0.8 with obs_score=0.3, should converge toward 0.3
        let mut w = 0.8f32;
        for _ in 0..200 {
            w = update_edge_weight(w, 0.3);
        }
        assert!((w - 0.3).abs() < 0.01, "failed to converge to 0.3: {w}");
    }

    // ── to_signal_map (regression) ────────────────────────────────────────────

    #[test]
    fn to_signal_map_active_task_is_one() {
        let s = ContextSignals { task_type: "crisis".into(), ..Default::default() };
        let m = s.to_signal_map();
        assert_eq!(m["task_crisis"], 1.0);
    }

    #[test]
    fn to_signal_map_inactive_tasks_are_zero() {
        let s = ContextSignals { task_type: "crisis".into(), ..Default::default() };
        let m = s.to_signal_map();
        for k in ["task_coding", "task_planning", "task_casual", "task_reflection"] {
            assert_eq!(m[k], 0.0, "{k} should be 0.0");
        }
    }

    #[test]
    fn to_signal_map_consistent_with_get_signal() {
        let s = ContextSignals {
            sentiment: 0.7,
            task_type: "planning".into(),
            correction_rate: 0.2,
            topic_shift: 0.5,
            energy: 0.9,
        };
        let m = s.to_signal_map();
        for (key, val) in &m {
            let direct = s.get_signal(key);
            assert!(
                (direct - val).abs() < f32::EPSILON,
                "mismatch for key '{key}': map={val}, get_signal={direct}"
            );
        }
    }
}
