use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Embedding, Node, Storage, VectorIndex};

/// Configuration for the write gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteGateConfig {
    pub enabled: bool,
    /// Cosine similarity above which a conflict/contradiction is flagged.
    pub conflict_threshold: f32,
    /// Cosine similarity above which a node is always rejected as a duplicate.
    pub duplicate_threshold: f32,
    pub min_title_length: usize,
    pub min_body_length: usize,
    /// Tags required when importance is at or above this value.
    pub require_tags_above_importance: f32,
    /// Enforce minimum body length for high-importance nodes.
    pub require_body_length_for_importance: bool,
    /// Per-kind threshold overrides.
    pub overrides: HashMap<String, KindOverrideConfig>,
}

impl Default for WriteGateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            conflict_threshold: 0.85,
            duplicate_threshold: 0.92,
            min_title_length: 10,
            min_body_length: 20,
            require_tags_above_importance: 0.7,
            require_body_length_for_importance: true,
            overrides: HashMap::new(),
        }
    }
}

/// Per-kind config overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KindOverrideConfig {
    pub min_body_length: Option<usize>,
    pub conflict_threshold: Option<f32>,
}

/// Which gate check produced a rejection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GateCheck {
    Substance,
    Specificity,
    Conflict,
}

impl std::fmt::Display for GateCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GateCheck::Substance => write!(f, "substance"),
            GateCheck::Specificity => write!(f, "specificity"),
            GateCheck::Conflict => write!(f, "conflict"),
        }
    }
}

/// Rejection details returned when a gate check fails.
#[derive(Debug, Clone)]
pub struct GateRejection {
    pub check: GateCheck,
    pub reason: String,
    pub suggestion: String,
    /// ID of the conflicting existing node (conflict check only).
    pub existing_node: Option<String>,
    /// Title of the conflicting existing node (conflict check only).
    pub existing_title: Option<String>,
}

/// Result of a single gate check.
#[derive(Debug)]
pub enum GateResult {
    Pass,
    Reject(GateRejection),
}

/// Stateless write gate — all checks are pure functions.
pub struct WriteGate;

impl WriteGate {
    /// Check 1: Substance — is this worth storing?
    pub fn check_substance(node: &Node, config: &WriteGateConfig) -> GateResult {
        let kind_str = node.kind.as_str();
        let min_body = config
            .overrides
            .get(kind_str)
            .and_then(|o| o.min_body_length)
            .unwrap_or(config.min_body_length);

        let title = &node.data.title;
        let body = &node.data.body;

        if title.len() < config.min_title_length {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: format!(
                    "Title too short ({} chars, minimum {})",
                    title.len(),
                    config.min_title_length
                ),
                suggestion: "Use a descriptive title that identifies the specific knowledge being stored".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if body.len() < min_body {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: format!(
                    "Body too short ({} chars, minimum {})",
                    body.len(),
                    min_body
                ),
                suggestion: "Add more context to make this useful as a standalone memory"
                    .to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if body.trim() == title.trim() {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: "Body is identical to title — no additional context".to_string(),
                suggestion: "Add detail in the body that expands on the title".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        let trimmed_body = body.trim();

        if is_pure_url(trimmed_body) {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: "Body is a bare URL with no context".to_string(),
                suggestion:
                    "Add a description of what this URL contains or why it matters".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if trimmed_body.split_whitespace().count() <= 1 {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: "Body is a single word — not enough context".to_string(),
                suggestion: "Add more context to make this useful as a standalone memory"
                    .to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if is_just_timestamp(trimmed_body) {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Substance,
                reason: "Body appears to be just a timestamp — no substantive content".to_string(),
                suggestion: "Add context about what the timestamp refers to".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        // Kind-specific checks
        let body_lower = body.to_lowercase();
        match kind_str {
            "decision" => {
                let decision_words = [
                    "decided", "chose", "will", "should", "use", "adopt", "switch", "selected",
                    "going to", "opted",
                ];
                if !decision_words.iter().any(|w| body_lower.contains(w)) {
                    return GateResult::Reject(GateRejection {
                        check: GateCheck::Substance,
                        reason: "Decision nodes must contain an action or choice (e.g., 'decided', 'chose', 'will use', 'should adopt')".to_string(),
                        suggestion: "Rewrite as a concrete decision: what was decided and why"
                            .to_string(),
                        existing_node: None,
                        existing_title: None,
                    });
                }
            }
            "fact" => {
                let hedges = ["i think", "maybe", "probably"];
                if hedges.iter().any(|h| body_lower.starts_with(h)) {
                    return GateResult::Reject(GateRejection {
                        check: GateCheck::Substance,
                        reason: "Fact nodes must not start with hedging language ('I think', 'maybe', 'probably') — use kind=observation instead".to_string(),
                        suggestion: "Either state as a confirmed fact or change kind to 'observation'".to_string(),
                        existing_node: None,
                        existing_title: None,
                    });
                }
            }
            "pattern" => {
                let pattern_words = [
                    "when",
                    "always",
                    "never",
                    "tends to",
                    "pattern",
                    "recurring",
                    "consistently",
                    "typically",
                    "usually",
                ];
                if !pattern_words.iter().any(|w| body_lower.contains(w)) {
                    return GateResult::Reject(GateRejection {
                        check: GateCheck::Substance,
                        reason: "Pattern nodes must reference a recurring behavior (e.g., 'when', 'always', 'tends to', 'pattern')".to_string(),
                        suggestion: "Describe the recurring behavior pattern explicitly"
                            .to_string(),
                        existing_node: None,
                        existing_title: None,
                    });
                }
            }
            _ => {}
        }

        GateResult::Pass
    }

    /// Check 2: Specificity — is this useful standalone?
    pub fn check_specificity(node: &Node, config: &WriteGateConfig) -> GateResult {
        let title = &node.data.title;
        let body = &node.data.body;
        let importance = node.importance;

        if has_unresolved_pronouns(title, body) {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Specificity,
                reason: "Body starts with an unresolved pronoun ('He', 'She', 'They', 'It') — the reference is ambiguous without conversation context".to_string(),
                suggestion: "Replace the pronoun with the actual entity name".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if has_unanchored_temporal(title, body) {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Specificity,
                reason: "Title or body uses a relative time reference without anchoring (e.g., 'yesterday', 'last week') — ambiguous outside original context".to_string(),
                suggestion: "Use a specific date or event anchor instead of relative time references".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        if config.require_body_length_for_importance {
            if importance >= 0.9 && body.len() < 100 {
                return GateResult::Reject(GateRejection {
                    check: GateCheck::Specificity,
                    reason: format!(
                        "High importance ({:.1}) but body is only {} chars",
                        importance,
                        body.len()
                    ),
                    suggestion: "Either add more detail or reduce importance".to_string(),
                    existing_node: None,
                    existing_title: None,
                });
            }
            if importance >= 0.8 && body.len() < 50 {
                return GateResult::Reject(GateRejection {
                    check: GateCheck::Specificity,
                    reason: format!(
                        "High importance ({:.1}) but body is only {} chars",
                        importance,
                        body.len()
                    ),
                    suggestion: "Either add more detail or reduce importance".to_string(),
                    existing_node: None,
                    existing_title: None,
                });
            }
        }

        if importance >= config.require_tags_above_importance && node.data.tags.is_empty() {
            return GateResult::Reject(GateRejection {
                check: GateCheck::Specificity,
                reason: format!(
                    "High importance ({:.1}) nodes should have tags for discoverability",
                    importance
                ),
                suggestion: "Add relevant tags to make this node findable".to_string(),
                existing_node: None,
                existing_title: None,
            });
        }

        GateResult::Pass
    }

    /// Check 3: Conflict — does this contradict existing knowledge?
    ///
    /// Requires a pre-computed embedding for the incoming node.
    /// Returns `GateResult::Pass` if the vector index is empty or search fails,
    /// so a search error never silently blocks writes.
    pub fn check_conflict<S: Storage, V: VectorIndex>(
        node: &Node,
        embedding: &Embedding,
        vector_index: &V,
        storage: &S,
        config: &WriteGateConfig,
    ) -> GateResult {
        let kind_str = node.kind.as_str();
        let conflict_threshold = config
            .overrides
            .get(kind_str)
            .and_then(|o| o.conflict_threshold)
            .unwrap_or(config.conflict_threshold);

        let results = match vector_index.search(embedding, 5, None) {
            Ok(r) => r,
            Err(_) => return GateResult::Pass,
        };

        for result in &results {
            let score = result.score;

            // Hard duplicate — always reject regardless of kind/agent
            if score > config.duplicate_threshold {
                if let Ok(Some(existing)) = storage.get_node(result.node_id) {
                    return GateResult::Reject(GateRejection {
                        check: GateCheck::Conflict,
                        reason: format!("Near-duplicate found (similarity: {:.2})", score),
                        suggestion: "Update the existing node instead of creating a duplicate"
                            .to_string(),
                        existing_node: Some(existing.id.to_string()),
                        existing_title: Some(existing.data.title.clone()),
                    });
                }
            }

            // Conflict threshold — same kind → flag
            if score > conflict_threshold {
                if let Ok(Some(existing)) = storage.get_node(result.node_id) {
                    let same_kind = existing.kind.as_str() == kind_str;
                    let same_agent = existing.source.agent == node.source.agent;

                    if same_kind && same_agent {
                        return GateResult::Reject(GateRejection {
                            check: GateCheck::Conflict,
                            reason: format!(
                                "Near-duplicate found (similarity: {:.2})",
                                score
                            ),
                            suggestion:
                                "Update the existing node instead of creating a duplicate"
                                    .to_string(),
                            existing_node: Some(existing.id.to_string()),
                            existing_title: Some(existing.data.title.clone()),
                        });
                    } else if same_kind {
                        return GateResult::Reject(GateRejection {
                            check: GateCheck::Conflict,
                            reason: format!(
                                "Potential contradiction with existing node (similarity: {:.2})",
                                score
                            ),
                            suggestion: "If this supersedes the existing node, use PATCH /nodes/:id or add a 'supersedes' edge".to_string(),
                            existing_node: Some(existing.id.to_string()),
                            existing_title: Some(existing.data.title.clone()),
                        });
                    }
                    // Different kind: related — log at call site, do not reject
                }
            }
        }

        GateResult::Pass
    }
}

// ── Heuristic helpers ─────────────────────────────────────────────────────────

fn is_pure_url(s: &str) -> bool {
    (s.starts_with("http://") || s.starts_with("https://")) && !s.contains(' ')
}

fn is_just_timestamp(s: &str) -> bool {
    let s = s.trim();
    // All-digit string long enough to be a Unix timestamp
    if s.len() >= 8 && s.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // ISO 8601: only accept if the *entire* string is date/datetime characters
    // Valid chars after YYYY-MM-DD: T, :, ., Z, +, -, digits, space (as T-separator only)
    if s.len() >= 10 {
        let bytes = s.as_bytes();
        if bytes[4] == b'-'
            && bytes[7] == b'-'
            && s[..4].bytes().all(|b| b.is_ascii_digit())
            && s[5..7].bytes().all(|b| b.is_ascii_digit())
            && s[8..10].bytes().all(|b| b.is_ascii_digit())
        {
            // Exact date only
            if s.len() == 10 {
                return true;
            }
            // Must be followed by T (ISO 8601 datetime), and the rest must look like a time
            if bytes[10] == b'T' {
                return s[11..].bytes().all(|b| {
                    b.is_ascii_digit()
                        || b == b':'
                        || b == b'.'
                        || b == b'Z'
                        || b == b'+'
                        || b == b'-'
                });
            }
        }
    }
    false
}

/// Returns true if the body starts with an unresolved third-person pronoun AND
/// the title doesn't appear to name the referent.
fn has_unresolved_pronouns(title: &str, body: &str) -> bool {
    let body_lower = body.trim_start().to_lowercase();
    let ambiguous_starts = ["he ", "she ", "they ", "it "];
    if !ambiguous_starts
        .iter()
        .any(|p| body_lower.starts_with(p))
    {
        return false;
    }

    // The title resolves the reference if it starts with or contains a proper
    // noun (heuristic: capitalised word that isn't a common article/pronoun).
    let stopwords = [
        "The", "A", "An", "This", "That", "These", "Those", "He", "She", "They", "It", "In",
        "On", "At", "For", "With",
    ];
    let title_has_proper_noun = title
        .split_whitespace()
        .any(|w| {
            w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !stopwords.contains(&w)
                && w.len() > 2
        });

    !title_has_proper_noun
}

/// Returns true if the title or the opening of the body uses an unanchored
/// relative time reference (e.g. "yesterday", "last week").
fn has_unanchored_temporal(title: &str, body: &str) -> bool {
    let relative_terms = [
        "yesterday",
        "last week",
        "last month",
        "last year",
        "this morning",
        "this afternoon",
        "this evening",
        "last night",
        "earlier today",
    ];

    let title_lower = title.to_lowercase();
    if relative_terms.iter().any(|t| title_lower.contains(t)) {
        return true;
    }

    // Only check the opening ~60 chars of the body to avoid false positives
    // in longer narrative content.
    let body_start = &body[..body.len().min(60)].to_lowercase();
    relative_terms.iter().any(|t| body_start.starts_with(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, Source};

    fn make_node(kind: &str, title: &str, body: &str, importance: f32) -> Node {
        let mut n = Node::new(
            NodeKind::new(kind).unwrap(),
            title.to_string(),
            body.to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            importance,
        );
        n.data.tags = vec!["test".to_string()];
        n
    }

    #[test]
    fn substance_rejects_short_title() {
        let node = make_node("fact", "Short", "This is a sufficiently long body.", 0.5);
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_rejects_short_body() {
        let node = make_node("fact", "A long enough title here", "Too short", 0.5);
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_rejects_body_equal_to_title() {
        let node = make_node(
            "fact",
            "Identical title and body",
            "Identical title and body",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_rejects_pure_url() {
        let node = make_node(
            "fact",
            "Some reference link here",
            "https://example.com/something",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_rejects_fact_with_hedging() {
        let node = make_node(
            "fact",
            "Something about databases",
            "I think postgres is better than mysql for this workload",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_rejects_decision_without_action() {
        let node = make_node(
            "decision",
            "Database selection for Cortex",
            "We looked at postgres, sqlite, and redb for the storage layer",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_passes_valid_decision() {
        let node = make_node(
            "decision",
            "Database selection for Cortex",
            "We decided to use redb for the storage layer due to its zero-copy mmap design",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Pass
        ));
    }

    #[test]
    fn substance_rejects_pattern_without_recurrence() {
        let node = make_node(
            "pattern",
            "Agent communication style",
            "Kai sends structured JSON responses to all downstream services",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn substance_passes_pattern_with_recurrence() {
        let node = make_node(
            "pattern",
            "Agent communication style",
            "Kai always sends structured JSON responses to downstream services",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_substance(&node, &config),
            GateResult::Pass
        ));
    }

    #[test]
    fn specificity_rejects_unresolved_pronoun() {
        let node = make_node(
            "fact",
            "yesterday meeting notes",
            "He decided to migrate the service to kubernetes next quarter",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_specificity(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn specificity_passes_when_title_resolves_pronoun() {
        let node = make_node(
            "fact",
            "Mike's decision on infrastructure",
            "He decided to migrate the service to kubernetes next quarter",
            0.5,
        );
        let config = WriteGateConfig::default();
        // Mike in title resolves "He" — should pass specificity
        assert!(matches!(
            WriteGate::check_specificity(&node, &config),
            GateResult::Pass
        ));
    }

    #[test]
    fn specificity_rejects_unanchored_temporal() {
        let node = make_node(
            "event",
            "Yesterday meeting notes",
            "We discussed the roadmap and assigned owners to each epic",
            0.5,
        );
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_specificity(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn specificity_rejects_high_importance_low_body() {
        let node = make_node(
            "fact",
            "Critical architectural constraint",
            "cortex-core must have zero network deps",
            0.9,
        );
        let mut config = WriteGateConfig::default();
        config.require_body_length_for_importance = true;
        // tags are set by make_node, so tag check passes; body length fails
        assert!(matches!(
            WriteGate::check_specificity(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn specificity_rejects_missing_tags_at_high_importance() {
        let mut node = make_node(
            "fact",
            "A long enough title here",
            "This is a sufficiently detailed body that explains the fact in enough context.",
            0.8,
        );
        node.data.tags.clear();
        let config = WriteGateConfig::default();
        assert!(matches!(
            WriteGate::check_specificity(&node, &config),
            GateResult::Reject(_)
        ));
    }

    #[test]
    fn is_pure_url_detection() {
        assert!(is_pure_url("https://example.com/path"));
        assert!(!is_pure_url("https://example.com see this page for details"));
        assert!(!is_pure_url("not a url at all"));
    }

    #[test]
    fn timestamp_detection() {
        assert!(is_just_timestamp("2024-01-15"));
        assert!(is_just_timestamp("2024-01-15T12:30:00"));
        assert!(is_just_timestamp("1700000000"));
        assert!(!is_just_timestamp("2024-01-15 was when the incident occurred"));
    }
}
