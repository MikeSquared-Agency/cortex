use crate::cli::{grpc_connect, print_edge_table, EdgeCommands, EdgeCreateArgs, EdgeListArgs};
use anyhow::Result;
use cortex_proto::*;

pub async fn run(cmd: EdgeCommands, server: &str) -> Result<()> {
    match cmd {
        EdgeCommands::Create(args) => create(args, server).await,
        EdgeCommands::List(args) => list(args, server).await,
    }
}

async fn create(args: EdgeCreateArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let req = CreateEdgeRequest {
        from_id: args.from,
        to_id: args.to,
        relation: args.relation,
        weight: args.weight,
    };

    let resp = client.create_edge(req).await?.into_inner();

    if args.format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "id": resp.id,
                "from_id": resp.from_id,
                "to_id": resp.to_id,
                "relation": resp.relation,
                "weight": resp.weight,
            })
        );
    } else {
        println!("Created edge {}", resp.id);
        println!(
            "  {} --[{}]--> {} (weight: {:.2})",
            resp.from_id, resp.relation, resp.to_id, resp.weight
        );
    }

    Ok(())
}

async fn list(args: EdgeListArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let resp = client
        .get_edges(GetEdgesRequest {
            node_id: args.node,
            direction: args.direction,
        })
        .await?
        .into_inner();

    if args.format == "json" {
        let edges: Vec<_> = resp
            .edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "from_id": e.from_id,
                    "to_id": e.to_id,
                    "relation": e.relation,
                    "weight": e.weight,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&edges)?);
    } else {
        print_edge_table(&resp.edges);
    }

    Ok(())
}
