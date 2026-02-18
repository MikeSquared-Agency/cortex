use cortex_core::{Node, NodeKind, RedbStorage, Source, Storage};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

fn create_test_storage() -> (RedbStorage, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.redb");
    let storage = RedbStorage::open(&db_path).unwrap();
    (storage, temp_dir)
}

fn create_test_node(kind: NodeKind, title: &str) -> Node {
    Node::new(
        kind,
        title.to_string(),
        "Benchmark test body content".to_string(),
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
        b.iter_with_setup(
            || {
                let (storage, _temp) = create_test_storage();
                let node = create_test_node(NodeKind::Fact, "Benchmark node");
                (storage, node)
            },
            |(storage, node)| {
                black_box(storage.put_node(&node).unwrap());
            },
        );
    });
}

fn bench_node_lookup(c: &mut Criterion) {
    c.bench_function("node lookup by id", |b| {
        b.iter_with_setup(
            || {
                let (storage, _temp) = create_test_storage();
                let node = create_test_node(NodeKind::Fact, "Lookup node");
                storage.put_node(&node).unwrap();
                (storage, node.id, _temp)
            },
            |(storage, id, _temp)| {
                black_box(storage.get_node(id).unwrap());
            },
        );
    });
}

fn bench_batch_insert_10k(c: &mut Criterion) {
    c.bench_function("batch insert 10k nodes", |b| {
        b.iter_with_setup(
            || {
                let (storage, _temp) = create_test_storage();
                let nodes: Vec<Node> = (0..10_000)
                    .map(|i| create_test_node(NodeKind::Observation, &format!("Node {}", i)))
                    .collect();
                (storage, nodes, _temp)
            },
            |(storage, nodes, _temp)| {
                black_box(storage.put_nodes_batch(&nodes).unwrap());
            },
        );
    });
}

criterion_group!(
    benches,
    bench_single_node_insert,
    bench_node_lookup,
    bench_batch_insert_10k
);
criterion_main!(benches);
