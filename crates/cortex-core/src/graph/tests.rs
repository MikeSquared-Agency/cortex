use crate::graph::*;
use crate::storage::{RedbStorage, Storage};
use crate::types::*;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn create_test_storage() -> (Arc<RedbStorage>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("graph_test.redb");
    let storage = Arc::new(RedbStorage::open(&db_path).unwrap());
    (storage, temp_dir)
}

fn create_test_node(kind: NodeKind, title: &str) -> Node {
    Node::new(
        kind,
        title.to_string(),
        "Test body".to_string(),
        Source {
            agent: "test".to_string(),
            session: None,
            channel: None,
        },
        0.5,
    )
}

fn create_test_edge(from: NodeId, to: NodeId, relation: Relation, weight: f32) -> Edge {
    Edge::new(
        from,
        to,
        relation,
        weight,
        EdgeProvenance::Manual {
            created_by: "test".to_string(),
        },
    )
}

/// Build a small test graph:
/// A -> B -> C
///  \-> D -> E
fn build_test_graph(storage: &RedbStorage) -> (Node, Node, Node, Node, Node) {
    let a = create_test_node(NodeKind::Decision, "Decision A");
    let b = create_test_node(NodeKind::Fact, "Fact B");
    let c = create_test_node(NodeKind::Observation, "Observation C");
    let d = create_test_node(NodeKind::Pattern, "Pattern D");
    let e = create_test_node(NodeKind::Goal, "Goal E");

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();
    storage.put_node(&d).unwrap();
    storage.put_node(&e).unwrap();

    let edge_ab = create_test_edge(a.id, b.id, Relation::LedTo, 0.9);
    let edge_bc = create_test_edge(b.id, c.id, Relation::InformedBy, 0.8);
    let edge_ad = create_test_edge(a.id, d.id, Relation::LedTo, 0.7);
    let edge_de = create_test_edge(d.id, e.id, Relation::AppliesTo, 0.6);

    storage.put_edge(&edge_ab).unwrap();
    storage.put_edge(&edge_bc).unwrap();
    storage.put_edge(&edge_ad).unwrap();
    storage.put_edge(&edge_de).unwrap();

    (a, b, c, d, e)
}

#[test]
fn test_bfs_traversal() {
    let (storage, _temp) = create_test_storage();
    let (a, b, c, d, e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Traverse from A, depth 2, outgoing
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(2),
            direction: TraversalDirection::Outgoing,
            strategy: TraversalStrategy::Bfs,
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    // Should include A, B, C, D (depth 0, 1, 2)
    assert!(result.nodes.contains_key(&a.id));
    assert!(result.nodes.contains_key(&b.id));
    assert!(result.nodes.contains_key(&c.id));
    assert!(result.nodes.contains_key(&d.id));

    // E is at depth 2 from D, but D is already at depth 1, so E would be at depth 2
    assert!(result.nodes.contains_key(&e.id));

    // Check depths
    assert_eq!(result.depths.get(&a.id), Some(&0));
    assert_eq!(result.depths.get(&b.id), Some(&1));
    assert_eq!(result.depths.get(&d.id), Some(&1));
    assert_eq!(result.depths.get(&c.id), Some(&2));
    assert_eq!(result.depths.get(&e.id), Some(&2));
}

#[test]
fn test_dfs_traversal() {
    let (storage, _temp) = create_test_storage();
    let (a, _, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // DFS should still find all nodes
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(3),
            direction: TraversalDirection::Outgoing,
            strategy: TraversalStrategy::Dfs,
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    assert_eq!(result.nodes.len(), 5); // All 5 nodes
}

#[test]
fn test_weighted_traversal() {
    let (storage, _temp) = create_test_storage();
    let (a, _, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Weighted traversal should prioritize higher-weight edges
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(2),
            direction: TraversalDirection::Outgoing,
            strategy: TraversalStrategy::Weighted,
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    assert!(result.nodes.len() > 0);
}

#[test]
fn test_relation_filter() {
    let (storage, _temp) = create_test_storage();
    let (a, b, c, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Only follow LedTo edges
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(2),
            direction: TraversalDirection::Outgoing,
            relation_filter: Some(vec![Relation::LedTo]),
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    // Should have A, B, D (connected by LedTo)
    // Should NOT have C (connected by InformedBy) or E (connected by AppliesTo)
    assert!(result.nodes.contains_key(&a.id));
    assert!(result.nodes.contains_key(&b.id));
    assert!(!result.nodes.contains_key(&c.id)); // Not connected by LedTo
}

#[test]
fn test_kind_filter() {
    let (storage, _temp) = create_test_storage();
    let (a, b, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Only return Facts
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(2),
            direction: TraversalDirection::Outgoing,
            kind_filter: Some(vec![NodeKind::Fact]),
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    // Should only have B (Fact)
    assert_eq!(result.nodes.len(), 1);
    assert!(result.nodes.contains_key(&b.id));
}

#[test]
fn test_min_weight_filter() {
    let (storage, _temp) = create_test_storage();
    let (a, b, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Only follow edges with weight >= 0.8
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: Some(2),
            direction: TraversalDirection::Outgoing,
            min_weight: Some(0.8),
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    // Should have A, B (edge weight 0.9)
    // Should not have D (edge weight 0.7) or its descendants
    assert!(result.nodes.contains_key(&a.id));
    assert!(result.nodes.contains_key(&b.id));
    // D has weight 0.7, so shouldn't be included
}

#[test]
fn test_result_limit() {
    let (storage, _temp) = create_test_storage();
    let (a, _, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Limit to 2 nodes
    let result = engine
        .traverse(TraversalRequest {
            start: vec![a.id],
            max_depth: None,
            direction: TraversalDirection::Outgoing,
            limit: Some(2),
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    assert_eq!(result.nodes.len(), 2);
    assert!(result.truncated);
}

#[test]
fn test_incoming_direction() {
    let (storage, _temp) = create_test_storage();
    let (_, _, c, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Traverse backwards from C
    let result = engine
        .traverse(TraversalRequest {
            start: vec![c.id],
            max_depth: Some(2),
            direction: TraversalDirection::Incoming,
            include_start: true,
            ..Default::default()
        })
        .unwrap();

    // Should have C, B, A (going backwards)
    assert!(result.nodes.len() >= 3);
}

#[test]
fn test_shortest_path() {
    let (storage, _temp) = create_test_storage();
    let (a, _, c, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Find path from A to C
    let result = engine
        .find_paths(PathRequest {
            from: a.id,
            to: c.id,
            max_paths: 1,
            ..Default::default()
        })
        .unwrap();

    assert_eq!(result.paths.len(), 1);
    let path = &result.paths[0];

    // Path should be A -> B -> C
    assert_eq!(path.nodes.len(), 3);
    assert_eq!(path.nodes[0], a.id);
    assert_eq!(path.nodes[2], c.id);
    assert_eq!(path.length, 2);
}

#[test]
fn test_no_path_exists() {
    let (storage, _temp) = create_test_storage();
    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    // No edges between them

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine
        .find_paths(PathRequest {
            from: a.id,
            to: b.id,
            max_paths: 1,
            ..Default::default()
        })
        .unwrap();

    assert_eq!(result.paths.len(), 0);
}

#[test]
fn test_neighbors() {
    let (storage, _temp) = create_test_storage();
    let (a, b, _, d, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Get outgoing neighbors of A
    let neighbors = engine
        .neighbors(a.id, TraversalDirection::Outgoing, None)
        .unwrap();

    assert_eq!(neighbors.len(), 2); // B and D

    let neighbor_ids: Vec<NodeId> = neighbors.iter().map(|(n, _)| n.id).collect();
    assert!(neighbor_ids.contains(&b.id));
    assert!(neighbor_ids.contains(&d.id));
}

#[test]
fn test_neighborhood() {
    let (storage, _temp) = create_test_storage();
    let (a, _, _, _, _) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Get 2-hop neighborhood
    let neighborhood = engine.neighborhood(a.id, 2).unwrap();

    assert!(neighborhood.nodes.len() >= 3);
}

#[test]
fn test_reachable() {
    let (storage, _temp) = create_test_storage();
    let (a, b, c, d, e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // All nodes reachable from A
    let reachable = engine
        .reachable(a.id, TraversalDirection::Outgoing)
        .unwrap();

    // Should reach B, C, D, E (not A itself, as include_start=false in reachable)
    assert!(reachable.contains(&b.id));
    assert!(reachable.contains(&c.id));
    assert!(reachable.contains(&d.id));
    assert!(reachable.contains(&e.id));
}

#[test]
fn test_roots_and_leaves() {
    let (storage, _temp) = create_test_storage();
    let (a, _, _c, _, _e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    // Roots: nodes with no incoming edges of a given relation
    let roots = engine.roots(Relation::LedTo).unwrap();
    assert!(roots.iter().any(|n| n.id == a.id)); // A has no incoming LedTo

    // Leaves: nodes with no outgoing edges of a given relation
    let _leaves = engine.leaves(Relation::LedTo).unwrap();
    // B and D have LedTo edges, so leaves should not include them
    // C and E do not have outgoing LedTo edges
}

#[test]
fn test_most_connected() {
    let (storage, _temp) = create_test_storage();
    build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    let most_connected = engine.most_connected(3).unwrap();

    assert!(most_connected.len() <= 3);
    // A has 2 outgoing, B has 1 in + 1 out, etc.
}

#[test]
fn test_find_cycles() {
    let (storage, _temp) = create_test_storage();

    // Create a cycle: A -> B -> C -> A
    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");
    let c = create_test_node(NodeKind::Fact, "C");

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();

    storage.put_edge(&create_test_edge(a.id, b.id, Relation::RelatedTo, 1.0)).unwrap();
    storage.put_edge(&create_test_edge(b.id, c.id, Relation::RelatedTo, 1.0)).unwrap();
    storage.put_edge(&create_test_edge(c.id, a.id, Relation::RelatedTo, 1.0)).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    let cycles = engine.find_cycles().unwrap();

    assert!(cycles.len() > 0); // Should detect the cycle
}

#[test]
fn test_components() {
    let (storage, _temp) = create_test_storage();

    // Create two disconnected components
    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");
    let c = create_test_node(NodeKind::Fact, "C");
    let d = create_test_node(NodeKind::Fact, "D");

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();
    storage.put_node(&d).unwrap();

    // Component 1: A - B
    storage.put_edge(&create_test_edge(a.id, b.id, Relation::RelatedTo, 1.0)).unwrap();

    // Component 2: C - D
    storage.put_edge(&create_test_edge(c.id, d.id, Relation::RelatedTo, 1.0)).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    let components = engine.components().unwrap();

    assert_eq!(components.len(), 2); // Two disconnected components
}

#[test]
fn test_subgraph_merge() {
    let mut sg1 = Subgraph::new();
    let mut sg2 = Subgraph::new();

    let id1 = Uuid::now_v7();
    let id2 = Uuid::now_v7();

    sg1.nodes.insert(id1, create_test_node(NodeKind::Fact, "Node 1"));
    sg1.depths.insert(id1, 0);

    sg2.nodes.insert(id2, create_test_node(NodeKind::Fact, "Node 2"));
    sg2.depths.insert(id2, 1);

    sg1.merge(sg2);

    assert_eq!(sg1.nodes.len(), 2);
    assert_eq!(sg1.depths.len(), 2);
}

// === Additional edge case and stress tests ===

#[test]
fn test_empty_graph_traversal() {
    let (storage, _temp) = create_test_storage();
    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![Uuid::now_v7()], // non-existent node
        max_depth: Some(3),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    assert!(result.nodes.is_empty());
}

#[test]
fn test_single_node_no_edges() {
    let (storage, _temp) = create_test_storage();
    let node = create_test_node(NodeKind::Fact, "Lonely");
    storage.put_node(&node).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![node.id],
        max_depth: Some(3),
        direction: TraversalDirection::Both,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert!(result.edges.is_empty());
}

#[test]
fn test_depth_zero_returns_only_start() {
    let (storage, _temp) = create_test_storage();
    let (a, _b, _c, _d, _e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![a.id],
        max_depth: Some(0),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert!(result.nodes.contains_key(&a.id));
}

#[test]
fn test_multiple_start_nodes() {
    let (storage, _temp) = create_test_storage();
    let (a, _b, c, _d, _e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![a.id, c.id],
        max_depth: Some(1),
        direction: TraversalDirection::Both,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    // Should include both starts and their neighbors
    assert!(result.nodes.contains_key(&a.id));
    assert!(result.nodes.contains_key(&c.id));
}

#[test]
fn test_edge_post_pass_correctness() {
    // Verify edges only connect nodes that are both in the result
    let (storage, _temp) = create_test_storage();
    let (a, _b, _c, _d, _e) = build_test_graph(&storage);

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![a.id],
        max_depth: Some(1),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    // Every edge in the result should have both endpoints in the node set
    for edge in &result.edges {
        assert!(result.nodes.contains_key(&edge.from),
            "Edge from {} not in result nodes", edge.from);
        assert!(result.nodes.contains_key(&edge.to),
            "Edge to {} not in result nodes", edge.to);
    }
}

#[test]
fn test_bidirectional_traversal_no_duplicates() {
    let (storage, _temp) = create_test_storage();

    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");
    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();

    // Edge in both directions
    storage.put_edge(&create_test_edge(a.id, b.id, Relation::RelatedTo, 1.0)).unwrap();
    storage.put_edge(&create_test_edge(b.id, a.id, Relation::RelatedTo, 1.0)).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    let result = engine.traverse(TraversalRequest {
        start: vec![a.id],
        max_depth: Some(3),
        direction: TraversalDirection::Both,
        strategy: TraversalStrategy::Bfs,
        include_start: true,
        ..Default::default()
    }).unwrap();

    // Should not visit nodes more than once
    assert_eq!(result.nodes.len(), 2);
}

#[test]
fn test_weighted_traversal_prefers_heavy_edges() {
    let (storage, _temp) = create_test_storage();

    // Build a deeper graph where weight preference matters:
    // root -> heavy(0.99) -> heavy_child
    // root -> light(0.01) -> light_child
    // With limit=3 (root + 2), weighted should explore heavy branch first
    let root = create_test_node(NodeKind::Decision, "Root");
    let heavy = create_test_node(NodeKind::Fact, "Heavy path");
    let light = create_test_node(NodeKind::Fact, "Light path");
    let heavy_child = create_test_node(NodeKind::Observation, "Heavy child");
    let light_child = create_test_node(NodeKind::Observation, "Light child");
    storage.put_node(&root).unwrap();
    storage.put_node(&heavy).unwrap();
    storage.put_node(&light).unwrap();
    storage.put_node(&heavy_child).unwrap();
    storage.put_node(&light_child).unwrap();

    storage.put_edge(&create_test_edge(root.id, heavy.id, Relation::LedTo, 0.99)).unwrap();
    storage.put_edge(&create_test_edge(root.id, light.id, Relation::LedTo, 0.01)).unwrap();
    storage.put_edge(&create_test_edge(heavy.id, heavy_child.id, Relation::LedTo, 0.99)).unwrap();
    storage.put_edge(&create_test_edge(light.id, light_child.id, Relation::LedTo, 0.01)).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    // Limit to 3: root + 2 others. Weighted should explore heavy branch first.
    let result = engine.traverse(TraversalRequest {
        start: vec![root.id],
        max_depth: Some(2),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Weighted,
        limit: Some(3),
        include_start: true,
        ..Default::default()
    }).unwrap();

    assert_eq!(result.nodes.len(), 3);
    assert!(result.nodes.contains_key(&root.id));
    assert!(result.nodes.contains_key(&heavy.id));
    // Third node should be heavy_child (weight 0.99) not light (weight 0.01)
    assert!(result.nodes.contains_key(&heavy_child.id));
}

#[test]
fn test_path_with_relation_filter() {
    let (storage, _temp) = create_test_storage();

    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");
    let c = create_test_node(NodeKind::Fact, "C");
    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();

    // Direct path A->C via LedTo
    storage.put_edge(&create_test_edge(a.id, c.id, Relation::LedTo, 1.0)).unwrap();
    // Indirect path A->B->C via RelatedTo
    storage.put_edge(&create_test_edge(a.id, b.id, Relation::RelatedTo, 1.0)).unwrap();
    storage.put_edge(&create_test_edge(b.id, c.id, Relation::RelatedTo, 1.0)).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());

    // Only follow RelatedTo — should find A->B->C, not A->C
    let result = engine.find_paths(PathRequest {
        from: a.id,
        to: c.id,
        relation_filter: Some(vec![Relation::RelatedTo]),
        max_paths: 1,
        ..Default::default()
    }).unwrap();

    assert_eq!(result.paths.len(), 1);
    assert_eq!(result.paths[0].length, 2); // A->B->C = 2 edges
}

#[test]
fn test_connected_components_isolated_nodes() {
    let (storage, _temp) = create_test_storage();

    // 3 isolated nodes = 3 components
    let a = create_test_node(NodeKind::Fact, "A");
    let b = create_test_node(NodeKind::Fact, "B");
    let c = create_test_node(NodeKind::Fact, "C");
    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();

    let engine = GraphEngineImpl::new(storage.clone());
    let components = engine.components().unwrap();
    assert_eq!(components.len(), 3);
}
