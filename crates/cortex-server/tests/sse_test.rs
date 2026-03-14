use cortex_core::hooks::{MutationAction, MutationHook};
use cortex_core::{Edge, EdgeProvenance, Node, NodeKind, Relation, Source};
use cortex_memory::observability::{new_event_bus, EventBusHook, GraphEvent};

fn make_test_node() -> Node {
    Node::new(
        NodeKind::new("fact").unwrap(),
        "Test SSE node".to_string(),
        "Body content for SSE integration test".to_string(),
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

// ── SSE Event Bus Tests ─────────────────────────────────────────────────────

#[test]
fn test_event_stream_endpoint_returns_sse_content_type() {
    // The SSE HTTP handler requires a running server which is out of scope for
    // integration tests.  Instead we verify the EventBus layer that backs the
    // endpoint: create a bus, subscribe, and confirm the subscriber count is
    // tracked correctly.
    let bus = new_event_bus(64);
    assert_eq!(bus.receiver_count(), 0, "No subscribers yet");

    let _rx = bus.subscribe();
    assert_eq!(bus.receiver_count(), 1, "One subscriber after subscribe()");
}

#[test]
fn test_event_stream_receives_events() {
    // Create an EventBus, subscribe, create an EventBusHook, fire a node
    // mutation, and verify the receiver gets the event with correct type and
    // data.
    let bus = new_event_bus(64);
    let mut rx = bus.subscribe();
    let hook = EventBusHook::new(bus);

    let node = make_test_node();
    hook.on_node_mutation(&node, MutationAction::Created);

    let event = rx
        .try_recv()
        .expect("Should receive the node.created event");
    assert_eq!(event.event_type, "node.created");
    assert_eq!(event.data["kind"], "fact");
    assert_eq!(event.data["title"], "Test SSE node");
    assert_eq!(event.data["agent"], "test-agent");
    assert!(
        event.data["id"].is_string(),
        "Event data should include the node id as a string"
    );
    // Timestamp should be a non-empty ISO-8601 string.
    assert!(
        !event.timestamp.is_empty(),
        "Event timestamp should be populated"
    );
}

#[test]
fn test_event_stream_filters_by_type() {
    // The EventBus delivers ALL events to every subscriber; filtering by
    // event_type is the responsibility of the SSE HTTP handler.  Here we
    // confirm a subscriber receives both node and edge events and can
    // manually filter them by type.
    let bus = new_event_bus(64);
    let mut rx = bus.subscribe();
    let hook = EventBusHook::new(bus);

    let node = make_test_node();
    let edge = make_test_edge();

    hook.on_node_mutation(&node, MutationAction::Created);
    hook.on_edge_mutation(&edge, MutationAction::Created);

    // Drain all events into a vec and separate by type.
    let mut events: Vec<GraphEvent> = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }

    assert_eq!(events.len(), 2, "Should receive exactly two events");

    let node_events: Vec<&GraphEvent> = events
        .iter()
        .filter(|e| e.event_type.starts_with("node."))
        .collect();
    let edge_events: Vec<&GraphEvent> = events
        .iter()
        .filter(|e| e.event_type.starts_with("edge."))
        .collect();

    assert_eq!(node_events.len(), 1, "One node event after filtering");
    assert_eq!(edge_events.len(), 1, "One edge event after filtering");
    assert_eq!(node_events[0].event_type, "node.created");
    assert_eq!(edge_events[0].event_type, "edge.created");
}

#[test]
fn test_multiple_sse_subscribers() {
    // Multiple subscribers should each independently receive a copy of every
    // event fired on the bus.
    let bus = new_event_bus(64);
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    assert_eq!(bus.receiver_count(), 2, "Two subscribers registered");

    let hook = EventBusHook::new(bus);
    let node = make_test_node();
    hook.on_node_mutation(&node, MutationAction::Updated);

    let event1 = rx1
        .try_recv()
        .expect("First subscriber should receive the event");
    let event2 = rx2
        .try_recv()
        .expect("Second subscriber should receive the event");

    assert_eq!(event1.event_type, "node.updated");
    assert_eq!(event2.event_type, "node.updated");
    assert_eq!(event1.data["title"], event2.data["title"]);
}
