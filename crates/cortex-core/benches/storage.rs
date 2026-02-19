use cortex_core::graph::*;
use cortex_core::storage::NodeFilter;
use cortex_core::storage::RedbStorage;
use cortex_core::storage::Storage;
use cortex_core::types::*;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn create_test_node(kind: NodeKind, title: &str) -> Node {
    Node::new(
        kind,
        title.to_string(),
        "Benchmark test body content for embedding quality testing".to_string(),
        Source {
            agent: "bench".to_string(),
            session: None,
            channel: None,
        },
        0.5,
    )
}

fn bench_single_node_insert(c: &mut Criterion) {
    c.bench_function("single node insert", |b| {
        b.iter_batched(
            || {
                let temp_dir = TempDir::new().unwrap();
                let db_path = temp_dir.path().join("bench.redb");
                let storage = RedbStorage::open(&db_path).unwrap();
                (storage, temp_dir)
            },
            |(storage, _temp)| {
                let node = create_test_node(NodeKind::new("fact").unwrap(), "Bench fact");
                storage.put_node(&node).unwrap();
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_node_lookup_by_id(c: &mut Criterion) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.redb");
    let storage = RedbStorage::open(&db_path).unwrap();

    let node = create_test_node(NodeKind::new("fact").unwrap(), "Lookup target");
    storage.put_node(&node).unwrap();
    let id = node.id;

    // Add 1000 other nodes
    for i in 0..1000 {
        let n = create_test_node(
            NodeKind::new("observation").unwrap(),
            &format!("Node {}", i),
        );
        storage.put_node(&n).unwrap();
    }

    c.bench_function("node lookup by ID (1k nodes)", |b| {
        b.iter(|| {
            storage.get_node(id).unwrap();
        });
    });
}

fn bench_batch_insert_1k(c: &mut Criterion) {
    c.bench_function("batch insert 1000 nodes", |b| {
        b.iter_batched(
            || {
                let temp_dir = TempDir::new().unwrap();
                let db_path = temp_dir.path().join("bench.redb");
                let storage = RedbStorage::open(&db_path).unwrap();
                let nodes: Vec<Node> = (0..1000)
                    .map(|i| {
                        create_test_node(
                            NodeKind::new("observation").unwrap(),
                            &format!("Node {}", i),
                        )
                    })
                    .collect();
                (storage, nodes, temp_dir)
            },
            |(storage, nodes, _temp)| {
                storage.put_nodes_batch(&nodes).unwrap();
            },
            BatchSize::LargeInput,
        );
    });
}

fn bench_filter_by_kind(c: &mut Criterion) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.redb");
    let storage = RedbStorage::open(&db_path).unwrap();

    // Insert 5000 nodes across different kinds
    let kinds = [
        NodeKind::new("fact").unwrap(),
        NodeKind::new("decision").unwrap(),
        NodeKind::new("event").unwrap(),
        NodeKind::new("pattern").unwrap(),
        NodeKind::new("observation").unwrap(),
    ];
    for i in 0..5000 {
        let kind = kinds[i % kinds.len()];
        let n = create_test_node(kind, &format!("Node {}", i));
        storage.put_node(&n).unwrap();
    }

    c.bench_function("filter by kind (5k nodes)", |b| {
        b.iter(|| {
            let filter = NodeFilter::new().with_kinds(vec![NodeKind::new("fact").unwrap()]);
            storage.list_nodes(filter).unwrap();
        });
    });
}

fn bench_bfs_traversal(c: &mut Criterion) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.redb");
    let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

    // Build a tree: root -> 10 children -> 10 grandchildren each = 111 nodes
    let root = create_test_node(NodeKind::new("decision").unwrap(), "Root");
    storage.put_node(&root).unwrap();
    let root_id = root.id;

    for i in 0..10 {
        let child = create_test_node(NodeKind::new("fact").unwrap(), &format!("Child {}", i));
        storage.put_node(&child).unwrap();
        let edge = Edge::new(
            root_id,
            child.id,
            Relation::new("led_to").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "bench".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        for j in 0..10 {
            let grandchild = create_test_node(
                NodeKind::new("observation").unwrap(),
                &format!("GC {}-{}", i, j),
            );
            storage.put_node(&grandchild).unwrap();
            let edge2 = Edge::new(
                child.id,
                grandchild.id,
                Relation::new("led_to").unwrap(),
                0.7,
                EdgeProvenance::Manual {
                    created_by: "bench".to_string(),
                },
            );
            storage.put_edge(&edge2).unwrap();
        }
    }

    let engine = GraphEngineImpl::new(storage.clone());

    c.bench_function("BFS 3-hop traversal (111 nodes, fanout 10)", |b| {
        b.iter(|| {
            engine
                .traverse(TraversalRequest {
                    start: vec![root_id],
                    max_depth: Some(3),
                    direction: TraversalDirection::Outgoing,
                    strategy: TraversalStrategy::Bfs,
                    include_start: true,
                    ..Default::default()
                })
                .unwrap();
        });
    });
}

fn bench_shortest_path(c: &mut Criterion) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.redb");
    let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

    // Build a chain: A -> B -> C -> ... -> Z (26 nodes)
    let mut nodes = Vec::new();
    for i in 0..26 {
        let n = create_test_node(NodeKind::new("fact").unwrap(), &format!("Chain {}", i));
        storage.put_node(&n).unwrap();
        nodes.push(n);
    }
    for i in 0..25 {
        let edge = Edge::new(
            nodes[i].id,
            nodes[i + 1].id,
            Relation::new("led_to").unwrap(),
            0.9,
            EdgeProvenance::Manual {
                created_by: "bench".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();
    }

    let engine = GraphEngineImpl::new(storage.clone());
    let start = nodes[0].id;
    let end = nodes[25].id;

    c.bench_function("shortest path (26-node chain)", |b| {
        b.iter(|| {
            engine
                .find_paths(PathRequest {
                    from: start,
                    to: end,
                    max_paths: 1,
                    ..Default::default()
                })
                .unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_single_node_insert,
    bench_node_lookup_by_id,
    bench_batch_insert_1k,
    bench_filter_by_kind,
    bench_bfs_traversal,
    bench_shortest_path,
);
criterion_main!(benches);
