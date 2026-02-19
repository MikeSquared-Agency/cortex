use anyhow::Result;
use cortex_proto::*;
use crate::cli::{SearchArgs, grpc_connect, truncate};

pub async fn run(args: SearchArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    if args.hybrid {
        let resp = client
            .hybrid_search(HybridSearchRequest {
                query: args.query,
                limit: args.limit,
                ..Default::default()
            })
            .await?
            .into_inner();

        if args.format == "json" {
            let results: Vec<_> = resp.results.iter().map(|r| {
                let node = r.node.as_ref().map(|n| serde_json::json!({
                    "id": n.id,
                    "kind": n.kind,
                    "title": n.title,
                })).unwrap_or(serde_json::json!(null));
                serde_json::json!({
                    "node": node,
                    "vector_score": r.vector_score,
                    "graph_score": r.graph_score,
                    "combined_score": r.combined_score,
                })
            }).collect();
            println!("{}", serde_json::to_string_pretty(&results)?);
        } else {
            println!("{:>4}  {:<6}  {:<6}  {:<6}  {:<12}  {}", "RANK", "COMB", "VEC", "GRAPH", "KIND", "TITLE");
            println!("{}", "─".repeat(80));
            for (i, r) in resp.results.iter().enumerate() {
                if let Some(node) = &r.node {
                    let title = truncate(&node.title, 40);
                    println!("{:>4}  {:.4}  {:.4}  {:.4}  {:<12}  {}",
                        i + 1, r.combined_score, r.vector_score, r.graph_score,
                        node.kind, title);
                }
            }
        }
    } else {
        let resp = client
            .similarity_search(SimilaritySearchRequest {
                query: args.query,
                limit: args.limit,
                ..Default::default()
            })
            .await?
            .into_inner();

        if args.format == "json" {
            let results: Vec<_> = resp.results.iter().map(|r| {
                let node = r.node.as_ref().map(|n| serde_json::json!({
                    "id": n.id,
                    "kind": n.kind,
                    "title": n.title,
                })).unwrap_or(serde_json::json!(null));
                serde_json::json!({"node": node, "score": r.score})
            }).collect();
            println!("{}", serde_json::to_string_pretty(&results)?);
        } else {
            println!("{:>4}  {:<6}  {:<12}  {:<36}  {}", "RANK", "SCORE", "KIND", "ID", "TITLE");
            println!("{}", "─".repeat(90));
            for (i, r) in resp.results.iter().enumerate() {
                if let Some(node) = &r.node {
                    let title = truncate(&node.title, 35);
                    println!("{:>4}  {:.4}  {:<12}  {:<36}  {}",
                        i + 1, r.score, node.kind, node.id, title);
                }
            }
        }
    }

    Ok(())
}
