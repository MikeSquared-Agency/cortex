use crate::cli::{grpc_connect, PathArgs, TraverseArgs};
use anyhow::Result;
use cortex_proto::*;

pub async fn run(args: TraverseArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let relation_filter = args.relation.map(|r| vec![r]).unwrap_or_default();

    let resp = client
        .traverse(TraverseRequest {
            start_ids: vec![args.id],
            max_depth: args.depth,
            direction: args.direction,
            relation_filter,
            limit: 200,
            ..Default::default()
        })
        .await?
        .into_inner();

    if args.format == "json" {
        let nodes: Vec<_> = resp
            .nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "kind": n.kind,
                    "title": n.title,
                })
            })
            .collect();
        let edges: Vec<_> = resp
            .edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "from": e.from_id,
                    "to": e.to_id,
                    "relation": e.relation,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "nodes": nodes,
                "edges": edges,
                "visited_count": resp.visited_count,
                "truncated": resp.truncated,
            }))?
        );
    } else {
        println!(
            "Subgraph: {} nodes, {} edges (visited: {}{})",
            resp.nodes.len(),
            resp.edges.len(),
            resp.visited_count,
            if resp.truncated { ", truncated" } else { "" }
        );
        println!();

        if !resp.nodes.is_empty() {
            println!("Nodes:");
            println!("{}", "─".repeat(70));
            for node in &resp.nodes {
                let depth = resp.depths.get(&node.id).copied().unwrap_or(0);
                let indent = "  ".repeat(depth as usize);
                println!(
                    "{}[{}] {} — {} ({})",
                    indent, depth, node.id, node.title, node.kind
                );
            }
        }

        if !resp.edges.is_empty() {
            println!();
            println!("Edges:");
            println!("{}", "─".repeat(70));
            for edge in &resp.edges {
                println!(
                    "  {} --[{}]--> {} ({:.2})",
                    &edge.from_id[..8],
                    edge.relation,
                    &edge.to_id[..8],
                    edge.weight
                );
            }
        }
    }

    Ok(())
}

pub async fn run_path(args: PathArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let resp = client
        .find_paths(FindPathsRequest {
            from_id: args.from,
            to_id: args.to,
            max_paths: 3,
            max_depth: args.max_hops,
        })
        .await?
        .into_inner();

    if args.format == "json" {
        let paths: Vec<_> = resp
            .paths
            .iter()
            .map(|p| {
                serde_json::json!({
                    "node_ids": p.node_ids,
                    "total_weight": p.total_weight,
                    "length": p.length,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&paths)?);
    } else {
        if resp.paths.is_empty() {
            println!("No path found between the two nodes.");
        } else {
            println!("Found {} path(s):", resp.paths.len());
            for (i, path) in resp.paths.iter().enumerate() {
                println!(
                    "  Path {}: {} hops, weight {:.2}",
                    i + 1,
                    path.length,
                    path.total_weight
                );
                println!("    {}", path.node_ids.join(" → "));
            }
        }
    }

    Ok(())
}
