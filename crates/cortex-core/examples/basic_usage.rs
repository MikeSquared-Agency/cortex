use cortex_core::{
    Edge, EdgeProvenance, Node, NodeFilter, NodeKind, RedbStorage, Relation, Source, Storage,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cortex Basic Usage Example ===\n");

    // 1. Create storage
    println!("1. Opening database...");
    let storage = RedbStorage::open("./data/example.redb")?;
    println!("   ✓ Database opened at ./data/example.redb\n");

    // 2. Create some nodes
    println!("2. Creating nodes...");

    let kai = Node::new(
        NodeKind::new("agent").unwrap(),
        "Kai - The Orchestrator".to_string(),
        "Kai is the primary orchestrator agent for Warren. Uses Opus 4.6 model. \
         Coordinates all other agents and manages high-level decision making."
            .to_string(),
        Source {
            agent: "system".to_string(),
            session: Some("bootstrap".to_string()),
            channel: None,
        },
        0.9, // High importance
    );

    let mut decision = Node::new(
        NodeKind::new("decision").unwrap(),
        "Use Rust for Cortex".to_string(),
        "Chose Rust for Cortex implementation because: (1) CPU-bound graph traversal \
         benefits from zero-cost abstractions, (2) Memory safety without GC for \
         predictable performance, (3) Single binary deployment simplifies ops."
            .to_string(),
        Source {
            agent: "kai".to_string(),
            session: Some("architecture-planning".to_string()),
            channel: Some("terminal".to_string()),
        },
        0.7,
    );
    decision.data.tags = vec!["architecture".to_string(), "tech-stack".to_string()];

    let fact = Node::new(
        NodeKind::new("fact").unwrap(),
        "Cortex uses redb for storage".to_string(),
        "redb is a pure-Rust embedded key-value store with ACID transactions, \
         MVCC, and zero-copy mmap reads. No external dependencies."
            .to_string(),
        Source {
            agent: "kai".to_string(),
            session: Some("architecture-planning".to_string()),
            channel: Some("terminal".to_string()),
        },
        0.6,
    );

    let observation = Node::new(
        NodeKind::new("observation").unwrap(),
        "Graph queries dominate CPU usage".to_string(),
        "During initial profiling, graph traversal operations accounted for \
         65% of CPU time in briefing synthesis."
            .to_string(),
        Source {
            agent: "monitoring".to_string(),
            session: None,
            channel: None,
        },
        0.4,
    );

    storage.put_node(&kai)?;
    storage.put_node(&decision)?;
    storage.put_node(&fact)?;
    storage.put_node(&observation)?;

    println!("   ✓ Created 4 nodes:");
    println!("     - Agent: {}", kai.data.title);
    println!("     - Decision: {}", decision.data.title);
    println!("     - Fact: {}", fact.data.title);
    println!("     - Observation: {}", observation.data.title);
    println!();

    // 3. Create relationships between nodes
    println!("3. Creating edges...");

    let edge1 = Edge::new(
        decision.id,
        observation.id,
        Relation::new("informed_by").unwrap(),
        0.8,
        EdgeProvenance::Manual {
            created_by: "kai".to_string(),
        },
    );

    let edge2 = Edge::new(
        decision.id,
        fact.id,
        Relation::new("led_to").unwrap(),
        0.9,
        EdgeProvenance::Manual {
            created_by: "kai".to_string(),
        },
    );

    let edge3 = Edge::new(
        fact.id,
        decision.id,
        Relation::new("applies_to").unwrap(),
        0.7,
        EdgeProvenance::AutoStructural {
            rule: "same-session".to_string(),
        },
    );

    storage.put_edge(&edge1)?;
    storage.put_edge(&edge2)?;
    storage.put_edge(&edge3)?;

    println!("   ✓ Created 3 edges:");
    println!(
        "     - {} --[InformedBy]--> {}",
        decision.data.title, observation.data.title
    );
    println!(
        "     - {} --[LedTo]--> {}",
        decision.data.title, fact.data.title
    );
    println!(
        "     - {} --[AppliesTo]--> {}",
        fact.data.title, decision.data.title
    );
    println!();

    // 4. Query nodes by kind
    println!("4. Querying nodes...");

    let filter = NodeFilter::new().with_kinds(vec![
        NodeKind::new("decision").unwrap(),
        NodeKind::new("fact").unwrap(),
    ]);
    let results = storage.list_nodes(filter)?;

    println!(
        "   ✓ Found {} nodes of kind Decision or Fact:",
        results.len()
    );
    for node in &results {
        println!("     - [{:?}] {}", node.kind, node.data.title);
    }
    println!();

    // 5. Query by tags
    let filter = NodeFilter::new().with_tags(vec!["architecture".to_string()]);
    let results = storage.list_nodes(filter)?;

    println!("   ✓ Found {} nodes tagged 'architecture':", results.len());
    for node in &results {
        println!("     - {}", node.data.title);
    }
    println!();

    // 6. Traverse edges
    println!("5. Traversing edges...");

    let outgoing = storage.edges_from(decision.id)?;
    println!(
        "   ✓ Edges from '{}': {}",
        decision.data.title,
        outgoing.len()
    );
    for edge in &outgoing {
        let to_node = storage.get_node(edge.to)?.unwrap();
        println!("     - --[{}]--> {}", edge.relation, to_node.data.title);
    }
    println!();

    let incoming = storage.edges_to(decision.id)?;
    println!(
        "   ✓ Edges to '{}': {}",
        decision.data.title,
        incoming.len()
    );
    for edge in &incoming {
        let from_node = storage.get_node(edge.from)?.unwrap();
        println!("     - {} --[{}]-->", from_node.data.title, edge.relation);
    }
    println!();

    // 7. Get statistics
    println!("6. Database statistics...");
    let stats = storage.stats()?;

    println!("   ✓ Total nodes: {}", stats.node_count);
    println!("   ✓ Total edges: {}", stats.edge_count);
    println!("   ✓ Nodes by kind:");
    for (kind, count) in &stats.node_counts_by_kind {
        println!("     - {:?}: {}", kind, count);
    }
    println!("   ✓ Database size: {} bytes", stats.db_size_bytes);
    println!();

    // 8. Update a node (record access)
    println!("7. Recording access...");
    let mut kai_node = storage.get_node(kai.id)?.unwrap();
    println!(
        "   Initial access count for '{}': {}",
        kai_node.data.title, kai_node.access_count
    );

    kai_node.record_access();
    storage.put_node(&kai_node)?;

    let kai_updated = storage.get_node(kai.id)?.unwrap();
    println!("   ✓ Updated access count: {}", kai_updated.access_count);
    println!();

    // 9. Soft delete
    println!("8. Testing soft delete...");
    storage.delete_node(observation.id)?;

    let deleted = storage.get_node(observation.id)?.unwrap();
    println!(
        "   ✓ Node '{}' deleted: {}",
        deleted.data.title, deleted.deleted
    );

    let filter = NodeFilter::new();
    let active_nodes = storage.list_nodes(filter.clone())?;
    println!(
        "   ✓ Active nodes (deleted excluded): {}",
        active_nodes.len()
    );

    let filter_with_deleted = filter.include_deleted();
    let all_nodes = storage.list_nodes(filter_with_deleted)?;
    println!("   ✓ All nodes (including deleted): {}", all_nodes.len());
    println!();

    println!("=== Example completed successfully! ===");

    Ok(())
}
