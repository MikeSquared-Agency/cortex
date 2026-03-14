//! Observability — SSE event streaming for real-time graph change notifications.

use cortex_core::hooks::{MutationAction, MutationHook};
use cortex_core::{Edge, Node};
use serde::Serialize;
use tokio::sync::broadcast;

/// A graph mutation event broadcast to SSE clients.
#[derive(Debug, Clone, Serialize)]
pub struct GraphEvent {
    /// Event type: "node.created", "node.updated", "node.deleted",
    /// "edge.created", "edge.updated", "edge.deleted"
    pub event_type: String,
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// Event payload
    pub data: serde_json::Value,
}

/// Broadcast channel type alias.
pub type EventBus = broadcast::Sender<GraphEvent>;

/// Creates a new event bus with the given capacity.
pub fn new_event_bus(capacity: usize) -> EventBus {
    let (tx, _rx) = broadcast::channel(capacity);
    tx
}

/// A MutationHook that bridges core mutations to the server's EventBus broadcast channel.
///
/// Register this hook so ALL mutations (gRPC, auto-linker, library mode) emit SSE events.
pub struct EventBusHook {
    bus: EventBus,
}

impl EventBusHook {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

    fn emit(&self, event: GraphEvent) {
        // Ignore send errors — no receivers means no one is listening (that's fine)
        let _ = self.bus.send(event);
    }
}

impl MutationHook for EventBusHook {
    fn on_node_mutation(&self, node: &Node, action: MutationAction) {
        let event_type = match action {
            MutationAction::Created => "node.created",
            MutationAction::Updated => "node.updated",
            MutationAction::Deleted => "node.deleted",
        };

        self.emit(GraphEvent {
            event_type: event_type.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: serde_json::json!({
                "id": node.id.to_string(),
                "kind": node.kind.as_str(),
                "title": node.data.title,
                "agent": node.source.agent,
                "importance": node.importance,
            }),
        });
    }

    fn on_edge_mutation(&self, edge: &Edge, action: MutationAction) {
        let event_type = match action {
            MutationAction::Created => "edge.created",
            MutationAction::Updated => "edge.updated",
            MutationAction::Deleted => "edge.deleted",
        };

        self.emit(GraphEvent {
            event_type: event_type.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: serde_json::json!({
                "id": edge.id.to_string(),
                "from": edge.from.to_string(),
                "to": edge.to.to_string(),
                "relation": edge.relation.as_str(),
                "weight": edge.weight,
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::{EdgeProvenance, NodeKind, Relation, Source};

    fn make_test_node() -> Node {
        Node::new(
            NodeKind::new("fact").unwrap(),
            "Test SSE node".to_string(),
            "Body content for SSE test".to_string(),
            Source {
                agent: "test-agent".to_string(),
                session: None,
                channel: None,
            },
            0.7,
        )
    }

    fn make_test_edge() -> Edge {
        Edge::new(
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            Relation::new("related_to").unwrap(),
            0.85,
            EdgeProvenance::Manual {
                created_by: "test-agent".to_string(),
            },
        )
    }

    #[test]
    fn test_event_bus_creation() {
        let bus = new_event_bus(64);
        // Should be able to subscribe
        let _rx = bus.subscribe();
        assert_eq!(bus.receiver_count(), 1);
    }

    #[test]
    fn test_event_bus_hook_emits_node_events() {
        let bus = new_event_bus(64);
        let mut rx = bus.subscribe();
        let hook = EventBusHook::new(bus);

        let node = make_test_node();
        hook.on_node_mutation(&node, MutationAction::Created);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type, "node.created");
        assert_eq!(event.data["kind"], "fact");
        assert_eq!(event.data["title"], "Test SSE node");
        assert_eq!(event.data["agent"], "test-agent");
    }

    #[test]
    fn test_event_bus_hook_emits_edge_events() {
        let bus = new_event_bus(64);
        let mut rx = bus.subscribe();
        let hook = EventBusHook::new(bus);

        let edge = make_test_edge();
        hook.on_edge_mutation(&edge, MutationAction::Created);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type, "edge.created");
        assert_eq!(event.data["relation"], "related_to");
    }

    #[test]
    fn test_event_bus_hook_no_receivers_is_ok() {
        let bus = new_event_bus(64);
        // No subscribers — emit should not panic
        let hook = EventBusHook::new(bus);
        let node = make_test_node();
        hook.on_node_mutation(&node, MutationAction::Created);
    }

    #[test]
    fn test_event_bus_hook_all_node_actions() {
        let bus = new_event_bus(64);
        let mut rx = bus.subscribe();
        let hook = EventBusHook::new(bus);
        let node = make_test_node();

        hook.on_node_mutation(&node, MutationAction::Created);
        hook.on_node_mutation(&node, MutationAction::Updated);
        hook.on_node_mutation(&node, MutationAction::Deleted);

        assert_eq!(rx.try_recv().unwrap().event_type, "node.created");
        assert_eq!(rx.try_recv().unwrap().event_type, "node.updated");
        assert_eq!(rx.try_recv().unwrap().event_type, "node.deleted");
    }

    #[test]
    fn test_event_bus_hook_all_edge_actions() {
        let bus = new_event_bus(64);
        let mut rx = bus.subscribe();
        let hook = EventBusHook::new(bus);
        let edge = make_test_edge();

        hook.on_edge_mutation(&edge, MutationAction::Created);
        hook.on_edge_mutation(&edge, MutationAction::Updated);
        hook.on_edge_mutation(&edge, MutationAction::Deleted);

        assert_eq!(rx.try_recv().unwrap().event_type, "edge.created");
        assert_eq!(rx.try_recv().unwrap().event_type, "edge.updated");
        assert_eq!(rx.try_recv().unwrap().event_type, "edge.deleted");
    }

    #[test]
    fn test_graph_event_serialization() {
        let event = GraphEvent {
            event_type: "node.created".to_string(),
            timestamp: "2026-01-01T00:00:00+00:00".to_string(),
            data: serde_json::json!({"id": "abc", "kind": "fact"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("node.created"));
        assert!(json.contains("event_type"));
        assert!(json.contains("timestamp"));
    }
}
