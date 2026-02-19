use cortex_core::{
    Edge, EdgeProvenance, GraphEngine, GraphEngineImpl, Node, NodeKind, PathRequest, RedbStorage,
    Relation, Source, Storage, TraversalDirection, TraversalRequest, TraversalStrategy,
};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cortex Graph Engine Example ===\n");

    // 1. Create storage and graph engine
    println!("1. Setting up graph engine...");
    let storage = Arc::new(RedbStorage::open("./data/graph_example.redb")?);
    let engine = GraphEngineImpl::new(storage.clone());
    println!("   ✓ Graph engine ready\n");

    // 2. Build a knowledge graph representing a decision chain
    println!("2. Building knowledge graph...");

    // Create nodes representing a real decision-making scenario
    let problem = Node::new(
        NodeKind::new("observation").unwrap(),
        "High latency in API responses".to_string(),
        "Users reporting slow response times, particularly during peak hours. \
         Average latency increased from 50ms to 300ms over the past week."
            .to_string(),
        Source {
            agent: "monitoring".to_string(),
            session: Some("incident-2024-01".to_string()),
            channel: Some("alerting".to_string()),
        },
        0.9,
    );

    let analysis = Node::new(
        NodeKind::new("fact").unwrap(),
        "Database queries not using indexes".to_string(),
        "Investigation revealed that recent schema changes removed critical indexes \
         on the users table, causing full table scans."
            .to_string(),
        Source {
            agent: "kai".to_string(),
            session: Some("incident-2024-01".to_string()),
            channel: None,
        },
        0.8,
    );

    let decision = Node::new(
        NodeKind::new("decision").unwrap(),
        "Add composite index on users table".to_string(),
        "Decision to add composite index on (created_at, status) columns. \
         Expected to reduce query time from 280ms to <20ms."
            .to_string(),
        Source {
            agent: "kai".to_string(),
            session: Some("incident-2024-01".to_string()),
            channel: None,
        },
        0.8,
    );

    let outcome = Node::new(
        NodeKind::new("observation").unwrap(),
        "Latency reduced to baseline".to_string(),
        "After index deployment, average latency dropped to 45ms. \
         P95 latency improved from 450ms to 65ms."
            .to_string(),
        Source {
            agent: "monitoring".to_string(),
            session: Some("incident-2024-01-followup".to_string()),
            channel: Some("alerting".to_string()),
        },
        0.7,
    );

    let pattern = Node::new(
        NodeKind::new("pattern").unwrap(),
        "Schema changes require index review".to_string(),
        "Pattern identified: schema migrations that modify table structure should \
         always include an index impact analysis before deployment."
            .to_string(),
        Source {
            agent: "kai".to_string(),
            session: Some("postmortem-2024-01".to_string()),
            channel: None,
        },
        0.9,
    );

    let goal = Node::new(
        NodeKind::new("goal").unwrap(),
        "Maintain API latency under 100ms".to_string(),
        "Ongoing goal to keep P95 API latency under 100ms for all endpoints.".to_string(),
        Source {
            agent: "warren".to_string(),
            session: None,
            channel: None,
        },
        0.95,
    );

    // Store nodes
    for node in [&problem, &analysis, &decision, &outcome, &pattern, &goal] {
        storage.put_node(node)?;
    }

    // Create edges representing relationships
    let edges = vec![
        Edge::new(
            problem.id,
            analysis.id,
            Relation::new("led_to").unwrap(),
            0.9,
            EdgeProvenance::Manual {
                created_by: "kai".to_string(),
            },
        ),
        Edge::new(
            analysis.id,
            decision.id,
            Relation::new("informed_by").unwrap(),
            0.95,
            EdgeProvenance::Manual {
                created_by: "kai".to_string(),
            },
        ),
        Edge::new(
            decision.id,
            outcome.id,
            Relation::new("led_to").unwrap(),
            0.85,
            EdgeProvenance::Manual {
                created_by: "kai".to_string(),
            },
        ),
        Edge::new(
            outcome.id,
            pattern.id,
            Relation::new("led_to").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "kai".to_string(),
            },
        ),
        Edge::new(
            pattern.id,
            goal.id,
            Relation::new("applies_to").unwrap(),
            0.7,
            EdgeProvenance::Manual {
                created_by: "kai".to_string(),
            },
        ),
        Edge::new(
            decision.id,
            goal.id,
            Relation::new("depends_on").unwrap(),
            0.6,
            EdgeProvenance::AutoStructural {
                rule: "decision-goal-link".to_string(),
            },
        ),
    ];

    for edge in &edges {
        storage.put_edge(edge)?;
    }

    println!("   ✓ Created {} nodes", 6);
    println!("   ✓ Created {} edges", edges.len());
    println!();

    // 3. Traversal: Find all downstream consequences of the problem
    println!("3. Traversal: What followed from the problem?");
    let consequences = engine.traverse(TraversalRequest {
        start: vec![problem.id],
        max_depth: Some(5),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        include_start: false,
        ..Default::default()
    })?;

    println!(
        "   ✓ Found {} nodes in the consequence chain:",
        consequences.nodes.len()
    );
    for (node_id, node) in &consequences.nodes {
        let depth = consequences.depths.get(node_id).unwrap();
        println!(
            "     [Depth {}] {:?}: {}",
            depth, node.kind, node.data.title
        );
    }
    println!();

    // 4. Path finding: Trace from problem to goal
    println!("4. Path Finding: How did we get from problem to goal?");
    let path_result = engine.find_paths(PathRequest {
        from: problem.id,
        to: goal.id,
        max_paths: 2,
        ..Default::default()
    })?;

    if !path_result.paths.is_empty() {
        let path = &path_result.paths[0];
        println!("   ✓ Found path with {} steps:", path.length);
        for (i, node_id) in path.nodes.iter().enumerate() {
            if let Some(node) = storage.get_node(*node_id)? {
                println!("     {}. {}", i + 1, node.data.title);
            }
        }
        println!("     Total weight: {:.2}", path.total_weight);
    }
    println!();

    // 5. Filtered traversal: Only follow decisions and facts
    println!("5. Filtered Traversal: Decision and fact chain");
    let decision_chain = engine.traverse(TraversalRequest {
        start: vec![problem.id],
        max_depth: Some(3),
        direction: TraversalDirection::Outgoing,
        kind_filter: Some(vec![
            NodeKind::new("decision").unwrap(),
            NodeKind::new("fact").unwrap(),
        ]),
        include_start: false,
        ..Default::default()
    })?;

    println!(
        "   ✓ Found {} decision/fact nodes:",
        decision_chain.nodes.len()
    );
    for node in decision_chain.nodes.values() {
        println!("     - {:?}: {}", node.kind, node.data.title);
    }
    println!();

    // 6. Relation filtering: Only follow LedTo edges
    println!("6. Relation Filtering: Causal chain (LedTo edges only)");
    let causal_chain = engine.traverse(TraversalRequest {
        start: vec![problem.id],
        max_depth: Some(5),
        direction: TraversalDirection::Outgoing,
        relation_filter: Some(vec![Relation::new("led_to").unwrap()]),
        include_start: true,
        ..Default::default()
    })?;

    println!("   ✓ Causal chain has {} nodes:", causal_chain.nodes.len());
    for (node_id, node) in &causal_chain.nodes {
        let depth = causal_chain.depths.get(node_id).unwrap();
        println!("     [Depth {}] {}", depth, node.data.title);
    }
    println!();

    // 7. Neighbors: What's directly connected to the decision?
    println!("7. Neighbors: What's directly connected to the decision?");
    let neighbors = engine.neighbors(decision.id, TraversalDirection::Both, None)?;

    println!("   ✓ Found {} neighbors:", neighbors.len());
    for (neighbor, edge) in &neighbors {
        let direction = if edge.from == decision.id {
            "outgoing"
        } else {
            "incoming"
        };
        println!(
            "     - [{}] {:?} via {:?}: {}",
            direction, neighbor.kind, edge.relation, neighbor.data.title
        );
    }
    println!();

    // 8. Roots and Leaves
    println!("8. Roots and Leaves:");
    let roots = engine.roots(Relation::new("led_to").unwrap())?;
    println!("   ✓ Root causes (no incoming LedTo):");
    for node in &roots {
        println!("     - {}", node.data.title);
    }

    let leaves = engine.leaves(Relation::new("led_to").unwrap())?;
    println!("   ✓ Terminal outcomes (no outgoing LedTo):");
    for node in &leaves {
        println!("     - {}", node.data.title);
    }
    println!();

    // 9. Most connected nodes
    println!("9. Network Analysis: Most connected nodes");
    let central = engine.most_connected(3)?;

    println!("   ✓ Top 3 most connected nodes:");
    for (i, (node, degree)) in central.iter().enumerate() {
        println!(
            "     {}. {} ({} connections)",
            i + 1,
            node.data.title,
            degree
        );
    }
    println!();

    // 10. Weighted traversal: Follow strongest connections
    println!("10. Weighted Traversal: Follow strongest connections");
    let strong_connections = engine.traverse(TraversalRequest {
        start: vec![problem.id],
        max_depth: Some(3),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Weighted,
        min_weight: Some(0.8),
        include_start: false,
        ..Default::default()
    })?;

    println!(
        "   ✓ Found {} nodes via strong edges (weight >= 0.8):",
        strong_connections.nodes.len()
    );
    for node in strong_connections.nodes.values() {
        println!("     - {}", node.data.title);
    }
    println!();

    // 11. Backward traversal: What led to the goal?
    println!("11. Backward Traversal: What led to the goal?");
    let influences = engine.traverse(TraversalRequest {
        start: vec![goal.id],
        max_depth: Some(3),
        direction: TraversalDirection::Incoming,
        include_start: false,
        ..Default::default()
    })?;

    println!("   ✓ Found {} influencing factors:", influences.nodes.len());
    for (node_id, node) in &influences.nodes {
        let depth = influences.depths.get(node_id).unwrap();
        println!(
            "     [Depth {}] {:?}: {}",
            depth, node.kind, node.data.title
        );
    }
    println!();

    // 12. Neighborhood: 2-hop context around the decision
    println!("12. Neighborhood: 2-hop context around decision");
    let neighborhood = engine.neighborhood(decision.id, 2)?;

    println!(
        "   ✓ 2-hop neighborhood has {} nodes and {} edges:",
        neighborhood.nodes.len(),
        neighborhood.edges.len()
    );
    for depth in 0..=2 {
        let at_depth = neighborhood.at_depth(depth);
        if !at_depth.is_empty() {
            println!("     Depth {}: {} nodes", depth, at_depth.len());
        }
    }
    println!();

    println!("=== Graph queries completed successfully! ===");

    Ok(())
}
