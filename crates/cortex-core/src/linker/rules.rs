use crate::types::{Edge, EdgeProvenance, Node, NodeId, Relation};
use crate::vector::SimilarityConfig;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;

/// Proposed edge from link rule evaluation
#[derive(Debug, Clone)]
pub struct ProposedEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub relation: Relation,
    pub weight: f32,
    pub provenance: EdgeProvenance,
}

impl ProposedEdge {
    pub fn to_edge(self) -> Edge {
        Edge::new(
            self.from,
            self.to,
            self.relation,
            self.weight,
            self.provenance,
        )
    }
}

/// Trait for link rules that evaluate potential edges
pub trait LinkRule {
    fn evaluate(
        &self,
        node: &Node,
        neighbor: &Node,
        score: f32,
        config: &SimilarityConfig,
    ) -> Option<ProposedEdge>;
}

/// Similarity-based link rule
pub struct SimilarityLinkRule;

impl LinkRule for SimilarityLinkRule {
    fn evaluate(
        &self,
        node: &Node,
        neighbor: &Node,
        score: f32,
        config: &SimilarityConfig,
    ) -> Option<ProposedEdge> {
        if score >= config.auto_link_threshold {
            Some(ProposedEdge {
                from: node.id,
                to: neighbor.id,
                relation: Relation::new("related_to").unwrap(),
                weight: score,
                provenance: EdgeProvenance::AutoSimilarity { score },
            })
        } else {
            None
        }
    }
}

/// Structural rules based on metadata
#[derive(Debug, Clone)]
pub enum StructuralRule {
    /// Same source agent → RelatedTo (weak).
    SameAgent { weight: f32 },

    /// Temporal proximity → RelatedTo.
    TemporalProximity { window: Duration, weight: f32 },

    /// Shared tags → RelatedTo.
    SharedTags { min_shared: usize, base_weight: f32 },

    /// Decision → Event in same session → LedTo.
    DecisionToEvent { weight: f32 },

    /// Observation → Pattern with same tags → InstanceOf.
    ObservationToPattern { min_similarity: f32, weight: f32 },

    /// New fact supersedes old fact with same title/tags.
    FactSupersedes { title_similarity: f32, weight: f32 },
}

impl Default for StructuralRule {
    fn default() -> Self {
        Self::SameAgent { weight: 0.3 }
    }
}

impl StructuralRule {
    pub fn same_agent() -> Self {
        Self::SameAgent { weight: 0.3 }
    }

    pub fn temporal_proximity() -> Self {
        Self::TemporalProximity {
            window: Duration::minutes(30),
            weight: 0.4,
        }
    }

    pub fn shared_tags() -> Self {
        Self::SharedTags {
            min_shared: 2,
            base_weight: 0.5,
        }
    }

    pub fn decision_to_event() -> Self {
        Self::DecisionToEvent { weight: 0.6 }
    }

    pub fn observation_to_pattern() -> Self {
        Self::ObservationToPattern {
            min_similarity: 0.7,
            weight: 0.7,
        }
    }

    pub fn fact_supersedes() -> Self {
        Self::FactSupersedes {
            title_similarity: 0.9,
            weight: 0.9,
        }
    }

    /// Evaluate this structural rule
    pub fn evaluate(&self, node: &Node, other: &Node, score: f32) -> Option<ProposedEdge> {
        match self {
            Self::SameAgent { weight } => {
                if node.source.agent == other.source.agent && node.id != other.id {
                    Some(ProposedEdge {
                        from: node.id,
                        to: other.id,
                        relation: Relation::new("related_to").unwrap(),
                        weight: *weight,
                        provenance: EdgeProvenance::AutoStructural {
                            rule: "same_agent".into(),
                        },
                    })
                } else {
                    None
                }
            }

            Self::TemporalProximity { window, weight } => {
                let time_diff = if node.created_at > other.created_at {
                    node.created_at - other.created_at
                } else {
                    other.created_at - node.created_at
                };

                if time_diff <= *window && node.id != other.id {
                    Some(ProposedEdge {
                        from: node.id,
                        to: other.id,
                        relation: Relation::new("related_to").unwrap(),
                        weight: *weight,
                        provenance: EdgeProvenance::AutoStructural {
                            rule: "temporal_proximity".into(),
                        },
                    })
                } else {
                    None
                }
            }

            Self::SharedTags {
                min_shared,
                base_weight,
            } => {
                let node_tags: HashSet<_> = node.data.tags.iter().collect();
                let other_tags: HashSet<_> = other.data.tags.iter().collect();
                let shared_count = node_tags.intersection(&other_tags).count();

                if shared_count >= *min_shared && node.id != other.id {
                    // Scale weight by number of shared tags
                    let scaled_weight =
                        base_weight * (1.0 + (shared_count - min_shared) as f32 * 0.1);
                    let clamped_weight = scaled_weight.min(1.0);

                    Some(ProposedEdge {
                        from: node.id,
                        to: other.id,
                        relation: Relation::new("related_to").unwrap(),
                        weight: clamped_weight,
                        provenance: EdgeProvenance::AutoStructural {
                            rule: "shared_tags".into(),
                        },
                    })
                } else {
                    None
                }
            }

            Self::DecisionToEvent { weight } => {
                if node.kind.as_str() == "decision"
                    && other.kind.as_str() == "event"
                    && node.source.session == other.source.session
                    && node.source.session.is_some()
                    && node.created_at < other.created_at
                {
                    Some(ProposedEdge {
                        from: node.id,
                        to: other.id,
                        relation: Relation::new("led_to").unwrap(),
                        weight: *weight,
                        provenance: EdgeProvenance::AutoStructural {
                            rule: "decision_to_event".into(),
                        },
                    })
                } else {
                    None
                }
            }

            Self::ObservationToPattern {
                min_similarity,
                weight,
            } => {
                if node.kind.as_str() == "observation"
                    && other.kind.as_str() == "pattern"
                    && score >= *min_similarity
                {
                    Some(ProposedEdge {
                        from: node.id,
                        to: other.id,
                        relation: Relation::new("instance_of").unwrap(),
                        weight: *weight,
                        provenance: EdgeProvenance::AutoStructural {
                            rule: "observation_to_pattern".into(),
                        },
                    })
                } else {
                    None
                }
            }

            Self::FactSupersedes {
                title_similarity,
                weight,
            } => {
                if node.kind.as_str() == "fact"
                    && other.kind.as_str() == "fact"
                    && node.created_at > other.created_at
                {
                    let title_score = self.simple_similarity(&node.data.title, &other.data.title);
                    if title_score >= *title_similarity {
                        Some(ProposedEdge {
                            from: node.id,
                            to: other.id,
                            relation: Relation::new("supersedes").unwrap(),
                            weight: *weight,
                            provenance: EdgeProvenance::AutoStructural {
                                rule: "fact_supersedes".into(),
                            },
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Simple string similarity (Jaccard on words)
    fn simple_similarity(&self, a: &str, b: &str) -> f32 {
        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();
        let words_a: HashSet<&str> = a_lower.split_whitespace().collect();
        let words_b: HashSet<&str> = b_lower.split_whitespace().collect();

        if words_a.is_empty() && words_b.is_empty() {
            return 1.0;
        }

        if words_a.is_empty() || words_b.is_empty() {
            return 0.0;
        }

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        intersection as f32 / union as f32
    }
}

/// Contradiction between two nodes
#[derive(Debug, Clone)]
pub struct Contradiction {
    pub node_a: NodeId,
    pub node_b: NodeId,
    pub similarity: f32,
    pub reason: String,
    pub suggested_resolution: Resolution,
    pub detected_at: DateTime<Utc>,
}

/// Suggested resolution for a contradiction
#[derive(Debug, Clone)]
pub enum Resolution {
    /// Supersede: newer replaces older
    Supersede { keep: NodeId, retire: NodeId },

    /// Manual review required
    ManualReview,
}

/// Detects contradictions between similar nodes
pub struct ContradictionDetector {
    threshold: f32,
}

impl Default for ContradictionDetector {
    fn default() -> Self {
        Self::new(0.80)
    }
}

impl ContradictionDetector {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    /// Check if two highly similar nodes contain contradictory information
    pub fn check(&self, a: &Node, b: &Node, similarity: f32) -> Option<Contradiction> {
        if similarity < self.threshold {
            return None;
        }

        // Check for negation patterns
        if self.has_negation_pattern(a, b) {
            let (newer, older) = if a.created_at > b.created_at {
                (a, b)
            } else {
                (b, a)
            };

            return Some(Contradiction {
                node_a: a.id,
                node_b: b.id,
                similarity,
                reason: "Negation pattern detected".into(),
                suggested_resolution: Resolution::Supersede {
                    keep: newer.id,
                    retire: older.id,
                },
                detected_at: Utc::now(),
            });
        }

        None
    }

    /// Detect negation patterns between two nodes
    fn has_negation_pattern(&self, a: &Node, b: &Node) -> bool {
        let negation_words = [
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

        // Check if one contains negation and the other doesn't
        let a_has_negation = negation_words.iter().any(|&word| a_text.contains(word));
        let b_has_negation = negation_words.iter().any(|&word| b_text.contains(word));

        a_has_negation != b_has_negation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodeKind, Source};

    fn create_test_node(kind: NodeKind, title: &str, body: &str) -> Node {
        Node::new(
            kind,
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

    #[test]
    fn test_similarity_link_rule() {
        let rule = SimilarityLinkRule;
        let config = SimilarityConfig::default();

        let node1 = create_test_node(NodeKind::new("fact").unwrap(), "Test 1", "Body 1");
        let node2 = create_test_node(NodeKind::new("fact").unwrap(), "Test 2", "Body 2");

        // Above threshold
        let result = rule.evaluate(&node1, &node2, 0.8, &config);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().relation,
            Relation::new("related_to").unwrap()
        );

        // Below threshold
        let result = rule.evaluate(&node1, &node2, 0.5, &config);
        assert!(result.is_none());
    }

    #[test]
    fn test_shared_tags_rule() {
        let rule = StructuralRule::shared_tags();

        let mut node1 = create_test_node(NodeKind::new("fact").unwrap(), "Test 1", "Body 1");
        node1.data.tags = vec!["rust".into(), "programming".into()];

        let mut node2 = create_test_node(NodeKind::new("fact").unwrap(), "Test 2", "Body 2");
        node2.data.tags = vec!["rust".into(), "programming".into(), "systems".into()];

        let result = rule.evaluate(&node1, &node2, 0.0);
        assert!(result.is_some());

        // Not enough shared tags
        node2.data.tags = vec!["python".into()];
        let result = rule.evaluate(&node1, &node2, 0.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_contradiction_detection() {
        let detector = ContradictionDetector::default();

        let node1 = create_test_node(
            NodeKind::new("fact").unwrap(),
            "System online",
            "The system is running",
        );
        let node2 = create_test_node(
            NodeKind::new("fact").unwrap(),
            "System offline",
            "The system is not running",
        );

        let result = detector.check(&node1, &node2, 0.85);
        assert!(result.is_some());

        let contradiction = result.unwrap();
        assert_eq!(contradiction.similarity, 0.85);
        assert!(contradiction.reason.contains("Negation"));
    }
}
