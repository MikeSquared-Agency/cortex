use cortex_core::*;
use std::sync::{Arc, RwLock as StdRwLock};
use tempfile::tempdir;

fn make_source(agent: &str) -> Source {
    Source {
        agent: agent.to_string(),
        session: None,
        channel: None,
    }
}

fn make_manual(created_by: &str) -> EdgeProvenance {
    EdgeProvenance::Manual {
        created_by: created_by.to_string(),
    }
}

// ── Storage Persistence ──────────────────────────────────────────────────────

#[test]
fn test_storage_persistence() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.redb");

    let node_id = {
        let storage = RedbStorage::open(&db_path).unwrap();
        let node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Persistence Test".to_string(),
            "Testing persistence across reopens".to_string(),
            make_source("test"),
            0.5,
        );
        let id = node.id;
        storage.put_node(&node).unwrap();
        id
    };

    // Reopen storage and verify data survived
    let storage = RedbStorage::open(&db_path).unwrap();
    let node = storage
        .get_node(node_id)
        .unwrap()
        .expect("Node should survive reopen");
    assert_eq!(node.data.title, "Persistence Test");
}

// ── Graph Traversal ──────────────────────────────────────────────────────────

#[test]
fn test_graph_bfs_traversal() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    // Create chain: A -> B -> C
    let node_a = Node::new(
        NodeKind::new("fact").unwrap(),
        "A".to_string(),
        "First".to_string(),
        make_source("test"),
        0.5,
    );
    let node_b = Node::new(
        NodeKind::new("fact").unwrap(),
        "B".to_string(),
        "Second".to_string(),
        make_source("test"),
        0.5,
    );
    let node_c = Node::new(
        NodeKind::new("fact").unwrap(),
        "C".to_string(),
        "Third".to_string(),
        make_source("test"),
        0.5,
    );

    storage.put_node(&node_a).unwrap();
    storage.put_node(&node_b).unwrap();
    storage.put_node(&node_c).unwrap();

    storage
        .put_edge(&Edge::new(
            node_a.id,
            node_b.id,
            Relation::new("informed_by").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();
    storage
        .put_edge(&Edge::new(
            node_b.id,
            node_c.id,
            Relation::new("informed_by").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();

    let request = TraversalRequest {
        start: vec![node_a.id],
        max_depth: Some(5),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        ..Default::default()
    };

    let subgraph = graph_engine.traverse(request).unwrap();

    assert!(
        subgraph.nodes.contains_key(&node_b.id),
        "B should be reachable from A"
    );
    assert!(
        subgraph.nodes.contains_key(&node_c.id),
        "C should be reachable from A"
    );
    assert_eq!(subgraph.depths[&node_b.id], 1);
    assert_eq!(subgraph.depths[&node_c.id], 2);
}

#[test]
fn test_graph_depth_limit() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let a = Node::new(
        NodeKind::new("fact").unwrap(),
        "A".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let b = Node::new(
        NodeKind::new("fact").unwrap(),
        "B".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let c = Node::new(
        NodeKind::new("fact").unwrap(),
        "C".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();

    storage
        .put_edge(&Edge::new(
            a.id,
            b.id,
            Relation::new("led_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();
    storage
        .put_edge(&Edge::new(
            b.id,
            c.id,
            Relation::new("led_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();

    // Depth 1 should reach B but not C
    let request = TraversalRequest {
        start: vec![a.id],
        max_depth: Some(1),
        direction: TraversalDirection::Outgoing,
        strategy: TraversalStrategy::Bfs,
        ..Default::default()
    };

    let subgraph = graph_engine.traverse(request).unwrap();
    assert!(subgraph.nodes.contains_key(&b.id));
    assert!(
        !subgraph.nodes.contains_key(&c.id),
        "C should be beyond depth limit"
    );
}

#[test]
fn test_find_paths() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let a = Node::new(
        NodeKind::new("fact").unwrap(),
        "A".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let b = Node::new(
        NodeKind::new("fact").unwrap(),
        "B".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let c = Node::new(
        NodeKind::new("fact").unwrap(),
        "C".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );

    storage.put_node(&a).unwrap();
    storage.put_node(&b).unwrap();
    storage.put_node(&c).unwrap();

    // A -> B -> C
    storage
        .put_edge(&Edge::new(
            a.id,
            b.id,
            Relation::new("led_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();
    storage
        .put_edge(&Edge::new(
            b.id,
            c.id,
            Relation::new("led_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();

    let path_req = PathRequest {
        from: a.id,
        to: c.id,
        max_paths: 1,
        max_length: Some(10),
        ..Default::default()
    };
    let result = graph_engine.find_paths(path_req).unwrap();

    assert!(!result.paths.is_empty(), "Should find path from A to C");
    assert_eq!(result.paths[0].nodes.len(), 3, "Path A->B->C has 3 nodes");
    assert_eq!(result.paths[0].nodes[0], a.id);
    assert_eq!(result.paths[0].nodes[2], c.id);
}

#[test]
fn test_graph_neighborhood() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let center = Node::new(
        NodeKind::new("agent").unwrap(),
        "Center".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let n1 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N1".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let n2 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N2".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let far = Node::new(
        NodeKind::new("fact").unwrap(),
        "Far".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );

    storage.put_node(&center).unwrap();
    storage.put_node(&n1).unwrap();
    storage.put_node(&n2).unwrap();
    storage.put_node(&far).unwrap();

    storage
        .put_edge(&Edge::new(
            center.id,
            n1.id,
            Relation::new("related_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();
    storage
        .put_edge(&Edge::new(
            center.id,
            n2.id,
            Relation::new("related_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();
    storage
        .put_edge(&Edge::new(
            n1.id,
            far.id,
            Relation::new("related_to").unwrap(),
            1.0,
            make_manual("test"),
        ))
        .unwrap();

    // neighborhood always traverses Both directions at the given depth
    let subgraph = graph_engine.neighborhood(center.id, 1).unwrap();

    assert!(subgraph.nodes.contains_key(&n1.id));
    assert!(subgraph.nodes.contains_key(&n2.id));
    assert!(
        !subgraph.nodes.contains_key(&far.id),
        "Far should be beyond depth 1"
    );
    assert_eq!(subgraph.edges.len(), 2);
}

// ── Vector Index ─────────────────────────────────────────────────────────────

#[test]
fn test_vector_index_rebuild() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));

    for i in 0..5 {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            format!("Test Node {}", i),
            format!("Body {}", i),
            make_source("test"),
            0.5,
        );
        let text = embedding_input(&node);
        let embedding = embedding_service.embed(&text).unwrap();
        node.embedding = Some(embedding.clone());
        storage.put_node(&node).unwrap();
        vector_index
            .write()
            .unwrap()
            .insert(node.id, &embedding)
            .unwrap();
    }

    // Rebuild should not lose data
    vector_index.write().unwrap().rebuild().unwrap();

    let query = embedding_service.embed("test query").unwrap();
    let results = vector_index
        .read()
        .unwrap()
        .search(&query, 5, None)
        .unwrap();
    assert!(!results.is_empty(), "Should find results after rebuild");
    assert!(results.len() <= 5);
}

#[test]
fn test_similarity_search_returns_relevant_results() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));

    // Add a semantically distinct set of nodes
    let topics = vec![
        (
            "Rust programming",
            "Rust is a systems programming language focused on safety",
        ),
        (
            "Python scripting",
            "Python is great for scripting and data science",
        ),
        (
            "Machine learning",
            "ML models learn patterns from training data",
        ),
    ];

    for (title, body) in topics {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            title.to_string(),
            body.to_string(),
            make_source("test"),
            0.5,
        );
        let text = embedding_input(&node);
        let emb = embedding_service.embed(&text).unwrap();
        node.embedding = Some(emb.clone());
        storage.put_node(&node).unwrap();
        vector_index.write().unwrap().insert(node.id, &emb).unwrap();
    }

    vector_index.write().unwrap().rebuild().unwrap();

    // Query for Rust — the Rust node should score highest
    let query_emb = embedding_service
        .embed("Rust systems programming safety")
        .unwrap();
    let results = vector_index
        .read()
        .unwrap()
        .search(&query_emb, 3, None)
        .unwrap();

    assert!(!results.is_empty());
    let top_node = storage.get_node(results[0].node_id).unwrap().unwrap();
    assert!(
        top_node.data.title.to_lowercase().contains("rust"),
        "Top result should be the Rust node, got: {}",
        top_node.data.title
    );
}

// ── Auto-Linker ──────────────────────────────────────────────────────────────

#[test]
fn test_auto_linker_creates_similarity_link() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let config = AutoLinkerConfig::default();
    let mut auto_linker = AutoLinker::new(
        storage.clone(),
        graph_engine,
        vector_index.clone(),
        embedding_service.clone(),
        config,
    )
    .unwrap();

    // Two highly similar nodes about Rust memory safety
    let mut node1 = Node::new(
        NodeKind::new("fact").unwrap(),
        "Rust is memory-safe".to_string(),
        "Rust provides memory safety without garbage collection".to_string(),
        make_source("test"),
        0.8,
    );
    let mut node2 = Node::new(
        NodeKind::new("fact").unwrap(),
        "Rust ensures memory safety".to_string(),
        "Memory safety is guaranteed by Rust's ownership system".to_string(),
        make_source("test"),
        0.8,
    );

    let emb1 = embedding_service.embed(&embedding_input(&node1)).unwrap();
    let emb2 = embedding_service.embed(&embedding_input(&node2)).unwrap();
    node1.embedding = Some(emb1.clone());
    node2.embedding = Some(emb2.clone());

    storage.put_node(&node1).unwrap();
    storage.put_node(&node2).unwrap();
    {
        let mut idx = vector_index.write().unwrap();
        idx.insert(node1.id, &emb1).unwrap();
        idx.insert(node2.id, &emb2).unwrap();
    }

    auto_linker.run_cycle().unwrap();

    let edges_from = storage.edges_from(node1.id).unwrap();
    let edges_to = storage.edges_to(node1.id).unwrap();
    assert!(
        edges_from.len() + edges_to.len() > 0,
        "Auto-linker should create a similarity edge between similar nodes"
    );
}

#[test]
fn test_auto_linker_metrics_update_after_cycle() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let mut auto_linker = AutoLinker::new(
        storage.clone(),
        graph_engine,
        vector_index,
        embedding_service,
        AutoLinkerConfig::default(),
    )
    .unwrap();

    assert_eq!(auto_linker.metrics().cycles, 0);
    auto_linker.run_cycle().unwrap();
    assert_eq!(
        auto_linker.metrics().cycles,
        1,
        "Cycle count should increment"
    );
}

// ── Edge Decay ───────────────────────────────────────────────────────────────

#[test]
fn test_edge_decay_preserves_recent_auto_edges() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let decay_engine = DecayEngine::new(storage.clone(), DecayConfig::default());

    let node1 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N1".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let node2 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N2".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    storage.put_node(&node1).unwrap();
    storage.put_node(&node2).unwrap();

    // Auto-similarity edge (subject to decay)
    let edge = Edge::new(
        node1.id,
        node2.id,
        Relation::new("related_to").unwrap(),
        1.0,
        EdgeProvenance::AutoSimilarity { score: 0.9 },
    );
    storage.put_edge(&edge).unwrap();

    // Just-created edges should survive decay
    let (pruned, deleted) = decay_engine.apply_decay(chrono::Utc::now()).unwrap();
    assert_eq!(
        pruned, 0,
        "Recently created auto-edges should not be pruned immediately"
    );
    assert_eq!(
        deleted, 0,
        "Recently created auto-edges should not be deleted immediately"
    );

    // Edge should still exist
    let retrieved = storage.get_edge(edge.id).unwrap();
    assert!(
        retrieved.is_some(),
        "Edge should still exist after decay pass"
    );
}

#[test]
fn test_edge_decay_exempts_manual_edges() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let decay_engine = DecayEngine::new(storage.clone(), DecayConfig::default());

    let n1 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N1".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    let n2 = Node::new(
        NodeKind::new("fact").unwrap(),
        "N2".to_string(),
        "".to_string(),
        make_source("test"),
        0.5,
    );
    storage.put_node(&n1).unwrap();
    storage.put_node(&n2).unwrap();

    let manual_edge = Edge::new(
        n1.id,
        n2.id,
        Relation::new("related_to").unwrap(),
        0.01,
        make_manual("test"),
    );
    storage.put_edge(&manual_edge).unwrap();

    // Even with weight below thresholds, manual edges are exempt
    let (_, deleted) = decay_engine.apply_decay(chrono::Utc::now()).unwrap();
    assert_eq!(
        deleted, 0,
        "Manual edges should be exempt from decay deletion"
    );

    let still_exists = storage.get_edge(manual_edge.id).unwrap();
    assert!(still_exists.is_some(), "Manual edge should survive decay");
}

// ── Hybrid Search ────────────────────────────────────────────────────────────

#[test]
fn test_hybrid_search_finds_relevant_nodes() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(RedbStorage::open(dir.path().join("test.redb")).unwrap());
    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let mut node = Node::new(
        NodeKind::new("fact").unwrap(),
        "Machine learning algorithms".to_string(),
        "Various algorithms used in machine learning include neural networks".to_string(),
        make_source("test"),
        0.8,
    );
    let emb = embedding_service.embed(&embedding_input(&node)).unwrap();
    node.embedding = Some(emb.clone());
    storage.put_node(&node).unwrap();
    vector_index.write().unwrap().insert(node.id, &emb).unwrap();

    // Arc<E> and Arc<G> implement EmbeddingService/GraphEngine via blanket impls.
    // RwLockVectorIndex wraps Arc<RwLock<V>> to implement VectorIndex.
    let hybrid = HybridSearch::new(
        storage.clone(),
        embedding_service.clone(),
        RwLockVectorIndex(vector_index.clone()),
        graph_engine.clone(),
    );

    let query = HybridQuery::new("machine learning".to_string()).with_limit(10);
    let results = hybrid.search(query).unwrap();

    assert!(!results.is_empty(), "Should find results for ML query");
    assert!(
        results[0].vector_score > 0.0,
        "Top result should have a positive vector score"
    );
}

// ── Config ───────────────────────────────────────────────────────────────────

#[test]
fn test_auto_linker_config_defaults_are_sane() {
    let config = AutoLinkerConfig::default();
    assert_eq!(config.interval.as_secs(), 60);
    assert_eq!(config.max_nodes_per_cycle, 500);
    assert_eq!(config.max_edges_per_cycle, 2000);
    assert!(config.run_on_startup);
    assert!(config.validate().is_ok());
}

#[test]
fn test_decay_config_defaults_are_sane() {
    let config = DecayConfig::default();
    assert!(config.daily_decay_rate > 0.0 && config.daily_decay_rate <= 1.0);
    assert!(config.prune_threshold > config.delete_threshold);
    assert!(config.exempt_manual);
    assert!(config.validate().is_ok());
}
