mod ingest;

pub use ingest::NatsIngest;

use async_nats::Subject;
use cortex_core::*;
use serde::Deserialize;

/// Warren event types that we ingest
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum WarrenEvent {
    #[serde(rename = "stage.advanced")]
    StageAdvanced {
        item_id: String,
        stage: String,
        previous_stage: Option<String>,
    },

    #[serde(rename = "item.completed")]
    ItemCompleted {
        item_id: String,
        title: String,
        evidence_count: u32,
    },

    #[serde(rename = "evidence.submitted")]
    EvidenceSubmitted {
        evidence_id: String,
        item_id: String,
        content: String,
        submitted_by: String,
    },

    #[serde(rename = "gate.approved")]
    GateApproved {
        gate_id: String,
        item_id: String,
        stage: String,
        approved_by: String,
    },

    #[serde(rename = "gate.rejected")]
    GateRejected {
        gate_id: String,
        item_id: String,
        stage: String,
        rejected_by: String,
        reason: String,
    },

    #[serde(rename = "interaction.created")]
    InteractionCreated {
        interaction_id: String,
        agent_id: String,
        content: String,
        channel: String,
    },

    #[serde(rename = "task.picked")]
    TaskPicked {
        task_id: String,
        item_id: String,
        picked_by: String,
    },

    #[serde(rename = "autonomy")]
    AutonomyEvent {
        agent_id: String,
        action: String,
        context: String,
    },

    #[serde(rename = "refinement")]
    RefinementEvent {
        refinement_id: String,
        content: String,
        agent_id: String,
    },
}

impl WarrenEvent {
    /// Convert Warren event to Cortex node
    pub fn to_node(&self, source_agent: &str) -> Node {
        match self {
            WarrenEvent::StageAdvanced {
                item_id,
                stage,
                previous_stage,
            } => {
                let title = format!("Item {} advanced to {}", item_id, stage);
                let body = format!(
                    "Item progressed from {} to {}",
                    previous_stage.as_deref().unwrap_or("start"),
                    stage
                );

                Node::new(
                    NodeKind::Event,
                    title,
                    body,
                    Source {
                        agent: source_agent.to_string(),
                        session: Some(item_id.clone()),
                        channel: Some("warren".to_string()),
                    },
                    0.6,
                )
            }

            WarrenEvent::ItemCompleted {
                item_id,
                title,
                evidence_count,
            } => {
                let body = format!(
                    "Item '{}' completed with {} pieces of evidence",
                    title, evidence_count
                );

                Node::new(
                    NodeKind::Event,
                    format!("Completed: {}", title),
                    body,
                    Source {
                        agent: source_agent.to_string(),
                        session: Some(item_id.clone()),
                        channel: Some("warren".to_string()),
                    },
                    0.8,
                )
            }

            WarrenEvent::EvidenceSubmitted {
                evidence_id,
                item_id,
                content,
                submitted_by,
            } => Node::new(
                NodeKind::Fact,
                format!("Evidence: {}", content.chars().take(50).collect::<String>()),
                content.clone(),
                Source {
                    agent: submitted_by.clone(),
                    session: Some(item_id.clone()),
                    channel: Some("warren".to_string()),
                },
                0.7,
            ),

            WarrenEvent::GateApproved {
                gate_id,
                item_id,
                stage,
                approved_by,
            } => Node::new(
                NodeKind::Decision,
                format!("Approved: {} gate for stage {}", gate_id, stage),
                format!("Gate approved by {}", approved_by),
                Source {
                    agent: approved_by.clone(),
                    session: Some(item_id.clone()),
                    channel: Some("warren".to_string()),
                },
                0.8,
            ),

            WarrenEvent::GateRejected {
                gate_id,
                item_id,
                stage,
                rejected_by,
                reason,
            } => Node::new(
                NodeKind::Decision,
                format!("Rejected: {} gate for stage {}", gate_id, stage),
                format!("Rejected by {}: {}", rejected_by, reason),
                Source {
                    agent: rejected_by.clone(),
                    session: Some(item_id.clone()),
                    channel: Some("warren".to_string()),
                },
                0.7,
            ),

            WarrenEvent::InteractionCreated {
                interaction_id,
                agent_id,
                content,
                channel,
            } => Node::new(
                NodeKind::Observation,
                format!("Interaction: {}", content.chars().take(50).collect::<String>()),
                content.clone(),
                Source {
                    agent: agent_id.clone(),
                    session: Some(interaction_id.clone()),
                    channel: Some(channel.clone()),
                },
                0.5,
            ),

            WarrenEvent::TaskPicked {
                task_id,
                item_id,
                picked_by,
            } => Node::new(
                NodeKind::Event,
                format!("Task {} picked", task_id),
                format!("Task picked by {} for item {}", picked_by, item_id),
                Source {
                    agent: picked_by.clone(),
                    session: Some(item_id.clone()),
                    channel: Some("warren".to_string()),
                },
                0.5,
            ),

            WarrenEvent::AutonomyEvent {
                agent_id,
                action,
                context,
            } => Node::new(
                NodeKind::Pattern,
                format!("Autonomy: {}", action),
                context.clone(),
                Source {
                    agent: agent_id.clone(),
                    session: None,
                    channel: Some("warren".to_string()),
                },
                0.7,
            ),

            WarrenEvent::RefinementEvent {
                refinement_id,
                content,
                agent_id,
            } => Node::new(
                NodeKind::Decision,
                format!("Refinement: {}", content.chars().take(50).collect::<String>()),
                content.clone(),
                Source {
                    agent: agent_id.clone(),
                    session: Some(refinement_id.clone()),
                    channel: Some("warren".to_string()),
                },
                0.6,
            ),
        }
    }
}

/// Parse NATS subject to determine event type
pub fn parse_subject(subject: &Subject) -> Option<&str> {
    let s = subject.as_str();
    if s.starts_with("warren.") {
        Some(&s[7..]) // Remove "warren." prefix
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::NodeKind;

    fn make_subject(s: &str) -> Subject {
        Subject::from(s.to_string())
    }

    // ── parse_subject ────────────────────────────────────────────────────────

    #[test]
    fn test_parse_subject_strips_warren_prefix() {
        assert_eq!(parse_subject(&make_subject("warren.stage.advanced")), Some("stage.advanced"));
        assert_eq!(parse_subject(&make_subject("warren.gate.approved")), Some("gate.approved"));
        assert_eq!(parse_subject(&make_subject("warren.autonomy.foo")), Some("autonomy.foo"));
    }

    #[test]
    fn test_parse_subject_non_warren_returns_none() {
        assert_eq!(parse_subject(&make_subject("other.event")), None);
        assert_eq!(parse_subject(&make_subject("warren")), None); // no trailing dot
        assert_eq!(parse_subject(&make_subject("")), None);
    }

    // ── WarrenEvent::to_node ─────────────────────────────────────────────────

    #[test]
    fn test_stage_advanced_maps_to_event_node() {
        let event = WarrenEvent::StageAdvanced {
            item_id: "item-123".to_string(),
            stage: "review".to_string(),
            previous_stage: Some("draft".to_string()),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Event);
        assert!(node.data.title.contains("item-123"));
        assert!(node.data.title.contains("review"));
        assert!(node.data.body.contains("draft"));
        assert_eq!(node.source.agent, "warren");
        assert_eq!(node.source.session, Some("item-123".to_string()));
        assert_eq!(node.source.channel, Some("warren".to_string()));
    }

    #[test]
    fn test_stage_advanced_without_previous_stage() {
        let event = WarrenEvent::StageAdvanced {
            item_id: "item-001".to_string(),
            stage: "draft".to_string(),
            previous_stage: None,
        };
        let node = event.to_node("warren");
        assert_eq!(node.kind, NodeKind::Event);
        // "start" used as fallback for missing previous_stage
        assert!(node.data.body.contains("start"));
    }

    #[test]
    fn test_item_completed_maps_to_event_node() {
        let event = WarrenEvent::ItemCompleted {
            item_id: "item-456".to_string(),
            title: "Build feature X".to_string(),
            evidence_count: 3,
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Event);
        assert!(node.data.title.contains("Build feature X"));
        assert!(node.data.body.contains("3 pieces of evidence"));
        assert!(node.importance >= 0.7);
        assert_eq!(node.source.session, Some("item-456".to_string()));
    }

    #[test]
    fn test_evidence_submitted_maps_to_fact_node() {
        let event = WarrenEvent::EvidenceSubmitted {
            evidence_id: "ev-789".to_string(),
            item_id: "item-456".to_string(),
            content: "The implementation meets all acceptance criteria".to_string(),
            submitted_by: "kai".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Fact);
        assert_eq!(node.source.agent, "kai");
        assert_eq!(node.source.session, Some("item-456".to_string()));
        assert!(node.data.body.contains("acceptance criteria"));
    }

    #[test]
    fn test_gate_approved_maps_to_decision_node() {
        let event = WarrenEvent::GateApproved {
            gate_id: "gate-001".to_string(),
            item_id: "item-123".to_string(),
            stage: "review".to_string(),
            approved_by: "mike".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Decision);
        assert_eq!(node.source.agent, "mike");
        assert!(node.data.body.contains("mike"));
        assert!(node.importance >= 0.7);
    }

    #[test]
    fn test_gate_rejected_maps_to_decision_node() {
        let event = WarrenEvent::GateRejected {
            gate_id: "gate-002".to_string(),
            item_id: "item-123".to_string(),
            stage: "review".to_string(),
            rejected_by: "mike".to_string(),
            reason: "Insufficient evidence provided".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Decision);
        assert!(node.data.body.contains("Insufficient evidence provided"));
        assert!(node.data.body.contains("mike"));
    }

    #[test]
    fn test_interaction_created_maps_to_observation_node() {
        let event = WarrenEvent::InteractionCreated {
            interaction_id: "int-001".to_string(),
            agent_id: "kai".to_string(),
            content: "User asked about deployment process".to_string(),
            channel: "slack".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Observation);
        assert_eq!(node.source.agent, "kai");
        assert_eq!(node.source.channel, Some("slack".to_string()));
        assert_eq!(node.source.session, Some("int-001".to_string()));
    }

    #[test]
    fn test_task_picked_maps_to_event_node() {
        let event = WarrenEvent::TaskPicked {
            task_id: "task-001".to_string(),
            item_id: "item-789".to_string(),
            picked_by: "dutybound".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Event);
        assert_eq!(node.source.agent, "dutybound");
        assert_eq!(node.source.session, Some("item-789".to_string()));
        assert!(node.data.title.contains("task-001"));
    }

    #[test]
    fn test_autonomy_event_maps_to_pattern_node() {
        let event = WarrenEvent::AutonomyEvent {
            agent_id: "dutybound".to_string(),
            action: "scheduled_review".to_string(),
            context: "Reviewing pending items at 09:00".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Pattern);
        assert_eq!(node.source.agent, "dutybound");
        assert!(node.data.title.contains("scheduled_review"));
        assert!(node.data.body.contains("09:00"));
    }

    #[test]
    fn test_refinement_event_maps_to_decision_node() {
        let event = WarrenEvent::RefinementEvent {
            refinement_id: "ref-001".to_string(),
            content: "Updated acceptance criteria for the API endpoint".to_string(),
            agent_id: "kai".to_string(),
        };
        let node = event.to_node("warren");

        assert_eq!(node.kind, NodeKind::Decision);
        assert_eq!(node.source.agent, "kai");
        assert_eq!(node.source.session, Some("ref-001".to_string()));
    }

    #[test]
    fn test_to_node_source_agent_passthrough() {
        // The source_agent parameter should always be set on the returned node
        // (some events override it with submitted_by/approved_by, others use the param)
        let event = WarrenEvent::StageAdvanced {
            item_id: "x".to_string(),
            stage: "y".to_string(),
            previous_stage: None,
        };
        let node = event.to_node("my-custom-agent");
        assert_eq!(node.source.agent, "my-custom-agent");
    }
}
