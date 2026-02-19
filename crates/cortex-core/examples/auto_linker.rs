//! Example: Auto-linker self-growing graph
//!
//! Run with: cargo run --example auto_linker
//! Note: First run downloads the embedding model (~30MB)

use cortex_core::graph::GraphEngineImpl;
use cortex_core::linker::{AutoLinker, AutoLinkerConfig};
use cortex_core::storage::{RedbStorage, Storage};
use cortex_core::types::*;
use cortex_core::vector::{EmbeddingService, FastEmbedService, HnswIndex, SimilarityConfig};
use std::sync::{Arc, RwLock};
use tempfile::TempDir;

fn main() {
    // Initialize logging
    env_logger::init();

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("auto_linker_demo.redb");

    let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

    println!("🧠 Cortex Auto-Linker Demo\n");

    // Create some knowledge nodes
    println!("Creating knowledge nodes...");

    let nodes = vec![
        Node::new(
            NodeKind::new("decision").unwrap(),
            "Use Rust for backend".into(),
            "Decided to use Rust for the Cortex backend due to performance requirements".into(),
            Source {
                agent: "kai".into(),
                session: Some("session-1".into()),
                channel: None,
            },
            0.8,
        ),
        Node::new(
            NodeKind::new("fact").unwrap(),
            "Rust is fast".into(),
            "Rust provides zero-cost abstractions and no garbage collector".into(),
            Source {
                agent: "kai".into(),
                session: Some("session-1".into()),
                channel: None,
            },
            0.7,
        ),
        Node::new(
            NodeKind::new("fact").unwrap(),
            "Rust is safe".into(),
            "Rust guarantees memory safety without garbage collection".into(),
            Source {
                agent: "kai".into(),
                session: Some("session-1".into()),
                channel: None,
            },
            0.7,
        ),
        Node::new(
            NodeKind::new("event").unwrap(),
            "Backend implementation started".into(),
            "Started implementing the Cortex backend in Rust".into(),
            Source {
                agent: "kai".into(),
                session: Some("session-1".into()),
                channel: None,
            },
            0.6,
        ),
        Node::new(
            NodeKind::new("observation").unwrap(),
            "Team velocity increased".into(),
            "Observed that team velocity increased after switching to Rust".into(),
            Source {
                agent: "kai".into(),
                session: Some("session-1".into()),
                channel: None,
            },
            0.5,
        ),
        Node::new(
            NodeKind::new("pattern").unwrap(),
            "Good tech choices boost morale".into(),
            "Pattern: When teams use technology they believe in, morale and velocity increase".into(),
            Source {
                agent: "kai".into(),
                session: None,
                channel: None,
            },
            0.9,
        ),
        Node::new(
            NodeKind::new("fact").unwrap(),
            "Python is slow".into(),
            "Python has a GIL and is slower for CPU-bound tasks".into(),
            Source {
                agent: "alex".into(),
                session: None,
                channel: None,
            },
            0.4,
        ),
    ];

    for node in &nodes {
        storage.put_node(node).unwrap();
    }

    println!("Created {} nodes\n", nodes.len());

    // Setup auto-linker
    println!("Initializing auto-linker...");

    let embedding_service = Arc::new(FastEmbedService::new().unwrap());
    let vector_index = Arc::new(RwLock::new(HnswIndex::new(embedding_service.dimension())));
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    let config = AutoLinkerConfig::new()
        .with_similarity(
            SimilarityConfig::new()
                .with_auto_link_threshold(0.65)
                .with_dedup_threshold(0.95)
                .with_contradiction_threshold(0.80),
        )
        .with_max_nodes_per_cycle(100)
        .with_max_edges_per_cycle(500);

    let mut linker = AutoLinker::new(
        storage.clone(),
        graph_engine.clone(),
        vector_index.clone(),
        embedding_service.clone(),
        config,
    )
    .unwrap();

    println!("Auto-linker initialized\n");

    // Run first cycle
    println!("Running auto-linker cycle 1...");
    linker.run_cycle().unwrap();

    let metrics = linker.metrics();
    println!("✓ Processed {} nodes", metrics.nodes_processed);
    println!("✓ Created {} edges", metrics.edges_created);
    println!("✓ Cycle took {:?}\n", metrics.last_cycle_duration);

    // Show created edges
    let stats = storage.stats().unwrap();
    println!("Graph statistics:");
    println!("  Nodes: {}", stats.node_count);
    println!("  Edges: {}", stats.edge_count);
    println!();

    if stats.edge_count > 0 {
        println!("Sample edges created:");
        // Collect edges by iterating over all nodes' outgoing edges
        let all_nodes = storage.list_nodes(cortex_core::storage::NodeFilter::new()).unwrap();
        let all_edges: Vec<_> = all_nodes.iter()
            .flat_map(|n| storage.edges_from(n.id).unwrap_or_default())
            .collect();

        for (i, edge) in all_edges.iter().take(5).enumerate() {
            let from = storage.get_node(edge.from).unwrap().unwrap();
            let to = storage.get_node(edge.to).unwrap().unwrap();

            println!(
                "  {}. [{}] {} → {} (weight: {:.2})",
                i + 1,
                format!("{:?}", edge.relation),
                from.data.title,
                to.data.title,
                edge.weight
            );

            println!(
                "     Provenance: {}",
                match &edge.provenance {
                    EdgeProvenance::Manual { created_by } => format!("Manual by {}", created_by),
                    EdgeProvenance::AutoSimilarity { score } =>
                        format!("Auto-similarity ({:.2})", score),
                    EdgeProvenance::AutoStructural { rule } => format!("Auto-structural ({})", rule),
                    EdgeProvenance::AutoContradiction { reason } =>
                        format!("Auto-contradiction ({})", reason),
                    EdgeProvenance::AutoDedup { similarity } =>
                        format!("Auto-dedup ({:.2})", similarity),
                    EdgeProvenance::Imported { source } => format!("Imported from {}", source),
                }
            );
        }
    }

    println!("\n✓ Auto-linker demo complete!");
    println!("\nThe graph is now self-organizing. In production, the auto-linker");
    println!("would run continuously in the background, discovering relationships");
    println!("and maintaining the graph structure automatically.");
}
