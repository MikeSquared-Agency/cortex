//! Example: Vector similarity search with Cortex
//!
//! Run with: cargo run --example vector_search
//! Note: First run downloads the embedding model (~30MB)

use cortex_core::storage::{RedbStorage, Storage};
use cortex_core::types::*;
use cortex_core::vector::{
    EmbeddingService, FastEmbedService, HnswIndex, VectorIndex, embedding_input,
};
use tempfile::TempDir;

fn main() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("example.redb");

    let storage = RedbStorage::open(&db_path).unwrap();

    // Create some knowledge nodes
    let nodes = vec![
        Node::new(
            NodeKind::Decision,
            "Use Rust for Cortex".into(),
            "Chose Rust over Go for the graph engine due to CPU-bound workload".into(),
            Source { agent: "kai".into(), session: None, channel: None },
            0.8,
        ),
        Node::new(
            NodeKind::Fact,
            "redb is an embedded database".into(),
            "redb is a pure Rust ACID key-value store with MVCC".into(),
            Source { agent: "kai".into(), session: None, channel: None },
            0.7,
        ),
        Node::new(
            NodeKind::Pattern,
            "Workers without integration instructions miss wiring".into(),
            "Briefings must explicitly say 'wire it in' or workers add functions without connecting them".into(),
            Source { agent: "kai".into(), session: None, channel: None },
            0.9,
        ),
        Node::new(
            NodeKind::Fact,
            "Pasta cooking time".into(),
            "Al dente pasta takes 8-10 minutes in boiling salted water".into(),
            Source { agent: "test".into(), session: None, channel: None },
            0.3,
        ),
    ];

    for node in &nodes {
        storage.put_node(node).unwrap();
    }

    // Initialize embedding service
    println!("Loading embedding model...");
    let embedding_service = FastEmbedService::new().unwrap();
    let mut index = HnswIndex::new(embedding_service.dimension());

    // Embed all nodes
    for node in &nodes {
        let text = embedding_input(node);
        let embedding = embedding_service.embed(&text).unwrap();
        index.insert(node.id, &embedding).unwrap();
    }
    index.rebuild().unwrap();

    // Search
    let query = "database technology choices";
    println!("\nSearching for: '{}'\n", query);

    let query_embedding = embedding_service.embed(query).unwrap();
    let results = index.search(&query_embedding, 3, None).unwrap();

    for (i, result) in results.iter().enumerate() {
        let node = storage.get_node(result.node_id).unwrap().unwrap();
        println!(
            "{}. [score: {:.3}] {} — {}",
            i + 1,
            result.score,
            node.data.title,
            format!("{:?}", node.kind)
        );
    }
}
