use crate::error::{CortexError, Result};
use crate::linker::rules::ProposedEdge;
use crate::types::{EdgeProvenance, Node, NodeKind, Relation};
use crate::vector::SimilarityConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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

    /// Embedding model name. Used for config change detection — if this changes,
    /// the linker resets its cursor and re-scans all nodes.
    pub embedding_model: String,

    /// User-defined structural linking rules. Default: empty (use legacy rules).
    pub rules: Vec<ConfigRule>,

    /// Whether to run the hardcoded legacy structural rules.
    /// None = auto: true when no config rules, false when config rules exist.
    pub legacy_rules_enabled: Option<bool>,
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
            embedding_model: "BAAI/bge-small-en-v1.5".into(),
            rules: Vec::new(),
            legacy_rules_enabled: None,
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

    pub fn with_embedding_model(mut self, model: String) -> Self {
        self.embedding_model = model;
        self
    }

    pub fn with_rules(mut self, rules: Vec<ConfigRule>) -> Self {
        self.rules = rules;
        self
    }

    pub fn with_legacy_rules_enabled(mut self, enabled: bool) -> Self {
        self.legacy_rules_enabled = Some(enabled);
        self
    }

    /// Whether legacy hardcoded structural rules should run.
    /// Auto-resolves: true if no config rules, false if config rules exist.
    pub fn use_legacy_rules(&self) -> bool {
        self.legacy_rules_enabled.unwrap_or(self.rules.is_empty())
    }

    pub fn validate(&self) -> Result<()> {
        self.similarity.validate()?;
        self.decay.validate()?;

        if self.max_nodes_per_cycle == 0 {
            return Err(CortexError::Validation(
                "max_nodes_per_cycle must be > 0".into(),
            ));
        }

        if self.max_edges_per_cycle == 0 {
            return Err(CortexError::Validation(
                "max_edges_per_cycle must be > 0".into(),
            ));
        }

        if self.max_edges_per_node == 0 {
            return Err(CortexError::Validation(
                "max_edges_per_node must be > 0".into(),
            ));
        }

        // Validate config rules
        let mut rule_names = HashSet::new();
        for rule in &self.rules {
            if !rule_names.insert(&rule.name) {
                return Err(CortexError::Validation(format!(
                    "Duplicate rule name: '{}'",
                    rule.name
                )));
            }
            rule.validate()?;
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
            return Err(CortexError::Validation(
                "daily_decay_rate must be between 0.0 and 1.0".into(),
            ));
        }

        if self.delete_threshold > self.prune_threshold {
            return Err(CortexError::Validation(
                "delete_threshold must be <= prune_threshold".into(),
            ));
        }

        if !(0.0..=1.0).contains(&self.importance_shield) {
            return Err(CortexError::Validation(
                "importance_shield must be between 0.0 and 1.0".into(),
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Configurable Rule Engine
// ---------------------------------------------------------------------------

fn default_weight() -> f32 {
    0.8
}

/// A user-configured structural linking rule, defined in cortex.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigRule {
    /// Human-readable name for logging and provenance.
    pub name: String,

    /// Kind of the source node (must match exactly).
    pub from_kind: String,

    /// Kind of the target node (must match exactly).
    pub to_kind: String,

    /// Relation to create on the edge.
    pub relation: String,

    /// Static weight for the edge (0.0-1.0). Default: 0.8.
    #[serde(default = "default_weight")]
    pub weight: f32,

    /// If true, use the cosine similarity score as edge weight instead of static weight.
    #[serde(default)]
    pub weight_from_score: bool,

    /// If true, also create the reverse edge (to -> from). Default: false.
    #[serde(default)]
    pub bidirectional: bool,

    /// Condition that must be satisfied for the rule to fire.
    pub condition: RuleCondition,
}

impl ConfigRule {
    /// Validate this rule's configuration.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(CortexError::Validation(
                "Rule name cannot be empty".into(),
            ));
        }
        // Validate kinds and relation using the same rules as NodeKind/Relation
        NodeKind::new(&self.from_kind).map_err(|e| {
            CortexError::Validation(format!("Rule '{}' from_kind: {}", self.name, e))
        })?;
        NodeKind::new(&self.to_kind).map_err(|e| {
            CortexError::Validation(format!("Rule '{}' to_kind: {}", self.name, e))
        })?;
        Relation::new(&self.relation).map_err(|e| {
            CortexError::Validation(format!("Rule '{}' relation: {}", self.name, e))
        })?;
        if !(0.0..=1.0).contains(&self.weight) {
            return Err(CortexError::Validation(format!(
                "Rule '{}' weight must be between 0.0 and 1.0",
                self.name
            )));
        }
        self.condition.validate(&self.name)?;
        Ok(())
    }

    /// Evaluate this rule for a given node pair. Returns proposed edges
    /// (may include a reverse edge if bidirectional).
    pub fn evaluate(
        &self,
        node: &Node,
        neighbor: &Node,
        similarity_score: f32,
    ) -> Vec<ProposedEdge> {
        let mut edges = Vec::new();

        // Kind gate: node must be from_kind, neighbor must be to_kind
        if node.kind.as_str() != self.from_kind || neighbor.kind.as_str() != self.to_kind {
            return edges;
        }

        // Self-edge check
        if node.id == neighbor.id {
            return edges;
        }

        // Evaluate condition
        if !self.condition.evaluate(node, neighbor, similarity_score) {
            return edges;
        }

        let weight = if self.weight_from_score {
            similarity_score
        } else {
            self.weight
        };

        // Relation was validated at config load time, so unwrap is safe
        let relation = match Relation::new(&self.relation) {
            Ok(r) => r,
            Err(_) => return edges,
        };

        edges.push(ProposedEdge {
            from: node.id,
            to: neighbor.id,
            relation: relation.clone(),
            weight,
            provenance: EdgeProvenance::AutoStructural {
                rule: self.name.clone(),
            },
        });

        if self.bidirectional {
            edges.push(ProposedEdge {
                from: neighbor.id,
                to: node.id,
                relation,
                weight,
                provenance: EdgeProvenance::AutoStructural {
                    rule: self.name.clone(),
                },
            });
        }

        edges
    }
}

/// Condition that determines when a config rule fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RuleCondition {
    /// Always fires for matching kinds.
    #[serde(rename = "always")]
    Always,

    /// Fires when cosine similarity >= threshold.
    #[serde(rename = "min_similarity")]
    MinSimilarity { threshold: f32 },

    /// Fires when nodes share >= min_shared tags.
    #[serde(rename = "shared_tags")]
    SharedTags { min_shared: usize },

    /// Fires when nodes have the same source agent.
    #[serde(rename = "same_agent")]
    SameAgent,

    /// Fires when nodes were created within window_minutes of each other.
    #[serde(rename = "temporal_proximity")]
    TemporalProximity { window_minutes: u64 },

    /// Fires when the from node was created after the to node.
    #[serde(rename = "newer_than")]
    NewerThan,

    /// Fires when from.body[field] (JSON) equals to.{match_field}.
    #[serde(rename = "body_field_ref")]
    BodyFieldRef { field: String, match_field: String },

    /// Fires when from.body[field] (JSON array) contains to.{match_field}.
    #[serde(rename = "body_field_contains")]
    BodyFieldContains { field: String, match_field: String },

    /// Fires when from has a tag matching "{tag_prefix}{to.title}".
    #[serde(rename = "tag_references_title")]
    TagReferencesTitle { tag_prefix: String },

    /// Fires when one node contains negation words the other doesn't.
    #[serde(rename = "negation_detected")]
    NegationDetected,
}

impl RuleCondition {
    /// Validate this condition's parameters.
    pub fn validate(&self, rule_name: &str) -> Result<()> {
        match self {
            Self::MinSimilarity { threshold } => {
                if !(0.0..=1.0).contains(threshold) {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' min_similarity threshold must be between 0.0 and 1.0",
                        rule_name
                    )));
                }
            }
            Self::SharedTags { min_shared } => {
                if *min_shared == 0 {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' min_shared must be > 0",
                        rule_name
                    )));
                }
            }
            Self::TemporalProximity { window_minutes } => {
                if *window_minutes == 0 {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' window_minutes must be > 0",
                        rule_name
                    )));
                }
            }
            Self::BodyFieldRef { field, match_field } => {
                if field.is_empty() || match_field.is_empty() {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' body_field_ref field and match_field cannot be empty",
                        rule_name
                    )));
                }
            }
            Self::BodyFieldContains { field, match_field } => {
                if field.is_empty() || match_field.is_empty() {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' body_field_contains field and match_field cannot be empty",
                        rule_name
                    )));
                }
            }
            Self::TagReferencesTitle { tag_prefix } => {
                if tag_prefix.is_empty() {
                    return Err(CortexError::Validation(format!(
                        "Rule '{}' tag_prefix cannot be empty",
                        rule_name
                    )));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Evaluate this condition for a given node pair.
    pub fn evaluate(&self, from: &Node, to: &Node, similarity_score: f32) -> bool {
        match self {
            Self::Always => true,
            Self::MinSimilarity { threshold } => similarity_score >= *threshold,
            Self::SharedTags { min_shared } => {
                let from_tags: HashSet<_> = from.data.tags.iter().collect();
                let to_tags: HashSet<_> = to.data.tags.iter().collect();
                from_tags.intersection(&to_tags).count() >= *min_shared
            }
            Self::SameAgent => from.source.agent == to.source.agent,
            Self::TemporalProximity { window_minutes } => {
                let diff = if from.created_at > to.created_at {
                    from.created_at - to.created_at
                } else {
                    to.created_at - from.created_at
                };
                diff <= chrono::Duration::minutes(*window_minutes as i64)
            }
            Self::NewerThan => from.created_at > to.created_at,
            Self::BodyFieldRef { field, match_field } => {
                check_body_field_ref(from, to, field, match_field)
            }
            Self::BodyFieldContains { field, match_field } => {
                check_body_field_contains(from, to, field, match_field)
            }
            Self::TagReferencesTitle { tag_prefix } => {
                let target = format!("{}{}", tag_prefix, to.data.title);
                from.data.tags.contains(&target)
            }
            Self::NegationDetected => has_negation_pattern(from, to),
        }
    }
}

/// Check if from.body[field] (JSON string) equals to.{match_field}.
fn check_body_field_ref(from: &Node, to: &Node, field: &str, match_field: &str) -> bool {
    let from_body = match serde_json::from_str::<serde_json::Value>(&from.data.body) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let field_value = match from_body.get(field).and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return false,
    };

    match resolve_match_field(to, match_field) {
        Some(ref target) => field_value == target.as_str(),
        None => false,
    }
}

/// Check if from.body[field] (JSON array) contains to.{match_field}.
fn check_body_field_contains(from: &Node, to: &Node, field: &str, match_field: &str) -> bool {
    let from_body = match serde_json::from_str::<serde_json::Value>(&from.data.body) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let arr = match from_body.get(field).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return false,
    };

    let target = match resolve_match_field(to, match_field) {
        Some(t) => t,
        None => return false,
    };

    arr.iter()
        .any(|elem| elem.as_str() == Some(target.as_str()))
}

/// Resolve a match_field reference on a node.
/// "title" → node.data.title, "id" → node.id, otherwise → node.body[field].
fn resolve_match_field(node: &Node, match_field: &str) -> Option<String> {
    match match_field {
        "title" => Some(node.data.title.clone()),
        "id" => Some(node.id.to_string()),
        _ => serde_json::from_str::<serde_json::Value>(&node.data.body)
            .ok()
            .and_then(|v| v.get(match_field)?.as_str().map(String::from)),
    }
}

/// Detect negation patterns between two nodes.
fn has_negation_pattern(a: &Node, b: &Node) -> bool {
    const NEGATION_WORDS: &[&str] = &[
        "not",
        "never",
        "no longer",
        "stopped",
        "removed",
        "deprecated",
        "replaced",
        "obsolete",
    ];

    let a_text = format!("{} {}", a.data.title, a.data.body).to_lowercase();
    let b_text = format!("{} {}", b.data.title, b.data.body).to_lowercase();

    let a_has = NEGATION_WORDS.iter().any(|&w| a_text.contains(w));
    let b_has = NEGATION_WORDS.iter().any(|&w| b_text.contains(w));

    a_has != b_has
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodeKind, Source};

    fn test_node(kind: &str, title: &str, body: &str) -> Node {
        Node::new(
            NodeKind::new(kind).unwrap(),
            title.to_string(),
            body.to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    fn test_node_with_agent(kind: &str, title: &str, agent: &str) -> Node {
        Node::new(
            NodeKind::new(kind).unwrap(),
            title.to_string(),
            String::new(),
            Source {
                agent: agent.to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    // --- Deserialization tests ---

    #[test]
    fn test_config_rule_deserialization_from_toml() {
        let toml_str = r#"
name = "test-rule"
from_kind = "experiment"
to_kind = "function"
relation = "targets"
weight = 0.8
condition = { type = "shared_tags", min_shared = 1 }
"#;
        let rule: ConfigRule = toml::from_str(toml_str).unwrap();
        assert_eq!(rule.name, "test-rule");
        assert_eq!(rule.from_kind, "experiment");
        assert_eq!(rule.to_kind, "function");
        assert_eq!(rule.relation, "targets");
        assert!((rule.weight - 0.8).abs() < f32::EPSILON);
        assert!(!rule.weight_from_score);
        assert!(!rule.bidirectional);
        assert!(matches!(
            rule.condition,
            RuleCondition::SharedTags { min_shared: 1 }
        ));
    }

    #[test]
    fn test_config_rule_array_deserialization() {
        let toml_str = r#"
[[rules]]
name = "rule-a"
from_kind = "fact"
to_kind = "fact"
relation = "supersedes"
weight = 0.9
condition = { type = "newer_than" }
bidirectional = false

[[rules]]
name = "rule-b"
from_kind = "function"
to_kind = "function"
relation = "similar_to"
weight_from_score = true
condition = { type = "min_similarity", threshold = 0.85 }
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            rules: Vec<ConfigRule>,
        }
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(w.rules.len(), 2);
        assert_eq!(w.rules[0].name, "rule-a");
        assert_eq!(w.rules[1].name, "rule-b");
        assert!(w.rules[1].weight_from_score);
    }

    #[test]
    fn test_all_condition_types_deserialize() {
        // TOML inline tables must be wrapped in a key
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Wrapper {
            condition: RuleCondition,
        }
        let cases = vec![
            r#"condition = { type = "always" }"#,
            r#"condition = { type = "min_similarity", threshold = 0.9 }"#,
            r#"condition = { type = "shared_tags", min_shared = 2 }"#,
            r#"condition = { type = "same_agent" }"#,
            r#"condition = { type = "temporal_proximity", window_minutes = 30 }"#,
            r#"condition = { type = "newer_than" }"#,
            r#"condition = { type = "body_field_ref", field = "source", match_field = "title" }"#,
            r#"condition = { type = "body_field_contains", field = "files", match_field = "title" }"#,
            r#"condition = { type = "tag_references_title", tag_prefix = "covers-" }"#,
            r#"condition = { type = "negation_detected" }"#,
        ];
        for case in cases {
            let result: std::result::Result<Wrapper, _> = toml::from_str(case);
            assert!(result.is_ok(), "Failed to deserialize: {}", case);
        }
    }

    // --- Validation tests ---

    #[test]
    fn test_config_rule_validation_valid() {
        let rule = ConfigRule {
            name: "valid-rule".into(),
            from_kind: "fact".into(),
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn test_config_rule_validation_empty_name() {
        let rule = ConfigRule {
            name: String::new(),
            from_kind: "fact".into(),
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };
        assert!(rule.validate().is_err());
    }

    #[test]
    fn test_config_rule_validation_invalid_kind() {
        let rule = ConfigRule {
            name: "bad-kind".into(),
            from_kind: "INVALID".into(), // uppercase not allowed
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };
        assert!(rule.validate().is_err());
    }

    #[test]
    fn test_config_rule_validation_invalid_relation() {
        let rule = ConfigRule {
            name: "bad-relation".into(),
            from_kind: "fact".into(),
            to_kind: "event".into(),
            relation: "Led-To".into(), // hyphens not allowed in relations
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };
        assert!(rule.validate().is_err());
    }

    #[test]
    fn test_config_rule_validation_weight_out_of_range() {
        let rule = ConfigRule {
            name: "bad-weight".into(),
            from_kind: "fact".into(),
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 1.5,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };
        assert!(rule.validate().is_err());
    }

    #[test]
    fn test_duplicate_rule_names_rejected() {
        let config = AutoLinkerConfig::new().with_rules(vec![
            ConfigRule {
                name: "duplicate".into(),
                from_kind: "fact".into(),
                to_kind: "fact".into(),
                relation: "related_to".into(),
                weight: 0.5,
                weight_from_score: false,
                bidirectional: false,
                condition: RuleCondition::Always,
            },
            ConfigRule {
                name: "duplicate".into(),
                from_kind: "event".into(),
                to_kind: "event".into(),
                relation: "related_to".into(),
                weight: 0.5,
                weight_from_score: false,
                bidirectional: false,
                condition: RuleCondition::Always,
            },
        ]);
        assert!(config.validate().is_err());
    }

    // --- Kind gating tests ---

    #[test]
    fn test_kind_gate_matches() {
        let rule = ConfigRule {
            name: "test".into(),
            from_kind: "experiment".into(),
            to_kind: "function".into(),
            relation: "targets".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };

        let experiment = test_node("experiment", "Exp 1", "body");
        let function = test_node("function", "Fn 1", "body");

        let edges = rule.evaluate(&experiment, &function, 0.9);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, experiment.id);
        assert_eq!(edges[0].to, function.id);
    }

    #[test]
    fn test_kind_gate_rejects_wrong_kinds() {
        let rule = ConfigRule {
            name: "test".into(),
            from_kind: "experiment".into(),
            to_kind: "function".into(),
            relation: "targets".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };

        let fact = test_node("fact", "Fact 1", "body");
        let function = test_node("function", "Fn 1", "body");

        // fact is not experiment, so rule should not fire
        let edges = rule.evaluate(&fact, &function, 0.9);
        assert!(edges.is_empty());

        // reversed order also should not fire
        let edges = rule.evaluate(&function, &fact, 0.9);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_self_edge_prevention() {
        let rule = ConfigRule {
            name: "test".into(),
            from_kind: "fact".into(),
            to_kind: "fact".into(),
            relation: "related_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };

        let node = test_node("fact", "Same node", "body");
        let edges = rule.evaluate(&node, &node, 1.0);
        assert!(edges.is_empty());
    }

    // --- Condition type tests ---

    #[test]
    fn test_condition_always() {
        let node = test_node("fact", "A", "body");
        let other = test_node("fact", "B", "body");
        assert!(RuleCondition::Always.evaluate(&node, &other, 0.0));
    }

    #[test]
    fn test_condition_min_similarity() {
        let node = test_node("fact", "A", "body");
        let other = test_node("fact", "B", "body");
        let cond = RuleCondition::MinSimilarity { threshold: 0.85 };

        assert!(cond.evaluate(&node, &other, 0.90));
        assert!(cond.evaluate(&node, &other, 0.85));
        assert!(!cond.evaluate(&node, &other, 0.80));
    }

    #[test]
    fn test_condition_shared_tags() {
        let mut node = test_node("fact", "A", "body");
        node.data.tags = vec!["rust".into(), "programming".into()];

        let mut other = test_node("fact", "B", "body");
        other.data.tags = vec!["rust".into(), "programming".into(), "systems".into()];

        let cond = RuleCondition::SharedTags { min_shared: 2 };
        assert!(cond.evaluate(&node, &other, 0.0));

        let cond = RuleCondition::SharedTags { min_shared: 3 };
        assert!(!cond.evaluate(&node, &other, 0.0));
    }

    #[test]
    fn test_condition_same_agent() {
        let node = test_node_with_agent("fact", "A", "agent-1");
        let same = test_node_with_agent("fact", "B", "agent-1");
        let diff = test_node_with_agent("fact", "C", "agent-2");

        assert!(RuleCondition::SameAgent.evaluate(&node, &same, 0.0));
        assert!(!RuleCondition::SameAgent.evaluate(&node, &diff, 0.0));
    }

    #[test]
    fn test_condition_temporal_proximity() {
        let node = test_node("fact", "A", "body");
        let mut near = test_node("fact", "B", "body");
        let mut far = test_node("fact", "C", "body");

        // near is 10 minutes apart
        near.created_at = node.created_at + chrono::Duration::minutes(10);
        // far is 60 minutes apart
        far.created_at = node.created_at + chrono::Duration::minutes(60);

        let cond = RuleCondition::TemporalProximity { window_minutes: 30 };
        assert!(cond.evaluate(&node, &near, 0.0));
        assert!(!cond.evaluate(&node, &far, 0.0));
    }

    #[test]
    fn test_condition_newer_than() {
        let node = test_node("fact", "A", "body");
        let mut older = test_node("fact", "B", "body");
        older.created_at = node.created_at - chrono::Duration::hours(1);

        assert!(RuleCondition::NewerThan.evaluate(&node, &older, 0.0));
        assert!(!RuleCondition::NewerThan.evaluate(&older, &node, 0.0));
    }

    #[test]
    fn test_condition_body_field_ref() {
        let from = test_node(
            "insight",
            "Insight 1",
            r#"{"source_experiment": "Exp Alpha"}"#,
        );
        let to_match = test_node("experiment", "Exp Alpha", "body");
        let to_no_match = test_node("experiment", "Exp Beta", "body");

        let cond = RuleCondition::BodyFieldRef {
            field: "source_experiment".into(),
            match_field: "title".into(),
        };

        assert!(cond.evaluate(&from, &to_match, 0.0));
        assert!(!cond.evaluate(&from, &to_no_match, 0.0));
    }

    #[test]
    fn test_condition_body_field_contains() {
        let from = test_node(
            "constraint",
            "Constraint 1",
            r#"{"applies_to_files": ["main.rs", "lib.rs", "config.rs"]}"#,
        );
        let to_match = test_node("file", "lib.rs", "body");
        let to_no_match = test_node("file", "util.rs", "body");

        let cond = RuleCondition::BodyFieldContains {
            field: "applies_to_files".into(),
            match_field: "title".into(),
        };

        assert!(cond.evaluate(&from, &to_match, 0.0));
        assert!(!cond.evaluate(&from, &to_no_match, 0.0));
    }

    #[test]
    fn test_condition_malformed_body_json() {
        let from = test_node("insight", "I1", "this is not json");
        let to = test_node("experiment", "Exp 1", "body");

        let cond = RuleCondition::BodyFieldRef {
            field: "source".into(),
            match_field: "title".into(),
        };
        // Should return false, not panic
        assert!(!cond.evaluate(&from, &to, 0.0));

        let cond = RuleCondition::BodyFieldContains {
            field: "files".into(),
            match_field: "title".into(),
        };
        assert!(!cond.evaluate(&from, &to, 0.0));
    }

    #[test]
    fn test_condition_tag_references_title() {
        let mut from = test_node("test-file", "test-main", "body");
        from.data.tags = vec!["covers-main".into()];

        let to_match = test_node("function", "main", "body");
        let to_no_match = test_node("function", "other", "body");

        let cond = RuleCondition::TagReferencesTitle {
            tag_prefix: "covers-".into(),
        };

        assert!(cond.evaluate(&from, &to_match, 0.0));
        assert!(!cond.evaluate(&from, &to_no_match, 0.0));
    }

    #[test]
    fn test_condition_negation_detected() {
        let positive = test_node("fact", "System online", "The system is running");
        let negative = test_node("fact", "System offline", "The system is not running");
        let also_positive = test_node("fact", "System up", "Everything works");

        assert!(RuleCondition::NegationDetected.evaluate(&positive, &negative, 0.0));
        assert!(!RuleCondition::NegationDetected.evaluate(&positive, &also_positive, 0.0));
    }

    // --- weight_from_score test ---

    #[test]
    fn test_weight_from_score() {
        let rule = ConfigRule {
            name: "score-weight".into(),
            from_kind: "function".into(),
            to_kind: "function".into(),
            relation: "similar_to".into(),
            weight: 0.5, // should be ignored
            weight_from_score: true,
            bidirectional: false,
            condition: RuleCondition::Always,
        };

        let node = test_node("function", "A", "body");
        let other = test_node("function", "B", "body");
        let edges = rule.evaluate(&node, &other, 0.92);

        assert_eq!(edges.len(), 1);
        assert!((edges[0].weight - 0.92).abs() < f32::EPSILON);
    }

    // --- Bidirectional test ---

    #[test]
    fn test_bidirectional_creates_both_edges() {
        let rule = ConfigRule {
            name: "bidir".into(),
            from_kind: "function".into(),
            to_kind: "function".into(),
            relation: "similar_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: true,
            condition: RuleCondition::Always,
        };

        let a = test_node("function", "Fn A", "body");
        let b = test_node("function", "Fn B", "body");
        let edges = rule.evaluate(&a, &b, 0.9);

        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].from, a.id);
        assert_eq!(edges[0].to, b.id);
        assert_eq!(edges[1].from, b.id);
        assert_eq!(edges[1].to, a.id);
    }

    #[test]
    fn test_non_bidirectional_creates_one_edge() {
        let rule = ConfigRule {
            name: "unidir".into(),
            from_kind: "fact".into(),
            to_kind: "fact".into(),
            relation: "supersedes".into(),
            weight: 0.9,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::NewerThan,
        };

        let newer = test_node("fact", "New fact", "body");
        let mut older = test_node("fact", "Old fact", "body");
        older.created_at = newer.created_at - chrono::Duration::hours(1);

        let edges = rule.evaluate(&newer, &older, 0.9);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from, newer.id);
        assert_eq!(edges[0].to, older.id);
    }

    // --- Legacy compat tests ---

    #[test]
    fn test_use_legacy_rules_default_no_config_rules() {
        let config = AutoLinkerConfig::new();
        assert!(config.use_legacy_rules());
    }

    #[test]
    fn test_use_legacy_rules_default_with_config_rules() {
        let config = AutoLinkerConfig::new().with_rules(vec![ConfigRule {
            name: "custom".into(),
            from_kind: "fact".into(),
            to_kind: "fact".into(),
            relation: "related_to".into(),
            weight: 0.5,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        }]);
        assert!(!config.use_legacy_rules());
    }

    #[test]
    fn test_use_legacy_rules_explicit_true() {
        let config = AutoLinkerConfig::new()
            .with_rules(vec![ConfigRule {
                name: "custom".into(),
                from_kind: "fact".into(),
                to_kind: "fact".into(),
                relation: "related_to".into(),
                weight: 0.5,
                weight_from_score: false,
                bidirectional: false,
                condition: RuleCondition::Always,
            }])
            .with_legacy_rules_enabled(true);
        assert!(config.use_legacy_rules());
    }

    #[test]
    fn test_use_legacy_rules_explicit_false() {
        let config = AutoLinkerConfig::new().with_legacy_rules_enabled(false);
        assert!(!config.use_legacy_rules());
    }

    // --- Provenance test ---

    #[test]
    fn test_provenance_contains_rule_name() {
        let rule = ConfigRule {
            name: "my-custom-rule".into(),
            from_kind: "fact".into(),
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 0.7,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::Always,
        };

        let fact = test_node("fact", "F1", "body");
        let event = test_node("event", "E1", "body");
        let edges = rule.evaluate(&fact, &event, 0.5);

        assert_eq!(edges.len(), 1);
        match &edges[0].provenance {
            EdgeProvenance::AutoStructural { rule } => {
                assert_eq!(rule, "my-custom-rule");
            }
            _ => panic!("Expected AutoStructural provenance"),
        }
    }
}
