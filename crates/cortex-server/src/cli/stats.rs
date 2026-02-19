use anyhow::Result;
use cortex_proto::StatsRequest;
use crate::cli::grpc_connect;

pub async fn run(server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let resp = client.stats(StatsRequest {}).await?.into_inner();

    let db_mb = resp.db_size_bytes as f64 / 1_048_576.0;

    println!();
    println!("Graph Overview");
    println!("{}", "─".repeat(50));
    println!("Nodes:   {:>8}", resp.node_count);

    let mut kinds: Vec<_> = resp.nodes_by_kind.iter().collect();
    kinds.sort_by_key(|(k, _)| k.as_str());
    for (kind, count) in &kinds {
        println!("  {:16} {:>8}", kind, count);
    }

    println!("Edges:   {:>8}", resp.edge_count);
    let mut relations: Vec<_> = resp.edges_by_relation.iter().collect();
    relations.sort_by_key(|(r, _)| r.as_str());
    for (rel, count) in &relations {
        println!("  {:16} {:>8}", rel, count);
    }

    println!("DB Size: {:>7.1} MB", db_mb);
    println!("{}", "─".repeat(50));
    println!();

    Ok(())
}
