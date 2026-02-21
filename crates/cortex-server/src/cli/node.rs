use crate::cli::{
    grpc_connect, print_node_table, NodeCommands, NodeCreateArgs, NodeDeleteArgs, NodeGetArgs,
    NodeListArgs, NodeStatsArgs,
};
use anyhow::Result;
use cortex_proto::*;
use prost_types;

pub async fn run(cmd: NodeCommands, server: &str) -> Result<()> {
    match cmd {
        NodeCommands::Create(args) => create(args, server).await,
        NodeCommands::Get(args) => get(args, server).await,
        NodeCommands::List(args) => list(args, server).await,
        NodeCommands::Delete(args) => delete(args, server).await,
        NodeCommands::Stats(args) => stats(args, server).await,
    }
}

async fn create(args: NodeCreateArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let body = if args.stdin {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s.trim().to_string()
    } else {
        args.body.unwrap_or_else(|| args.title.clone())
    };

    let req = CreateNodeRequest {
        kind: args.kind,
        title: args.title,
        body,
        importance: args.importance,
        tags: args.tags,
        source_agent: "cli".into(),
        ..Default::default()
    };

    let resp = client.create_node(req).await?.into_inner();

    if args.format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "id": resp.id,
                "kind": resp.kind,
                "title": resp.title,
                "importance": resp.importance,
            })
        );
    } else {
        println!("Created node {}", resp.id);
        print_node_detail(&resp);
    }

    Ok(())
}

async fn get(args: NodeGetArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;
    let resp = client
        .get_node(GetNodeRequest { id: args.id })
        .await?
        .into_inner();

    if args.format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "id": resp.id,
                "kind": resp.kind,
                "title": resp.title,
                "body": resp.body,
                "importance": resp.importance,
                "tags": resp.tags,
                "source_agent": resp.source_agent,
                "access_count": resp.access_count,
                "has_embedding": resp.has_embedding,
            })
        );
    } else {
        print_node_detail(&resp);
    }

    Ok(())
}

async fn list(args: NodeListArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let kind_filter = args.kind.map(|k| vec![k]).unwrap_or_default();
    let source_agent = args.source.unwrap_or_default();

    let resp = client
        .list_nodes(ListNodesRequest {
            kind_filter,
            source_agent,
            limit: args.limit,
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
                    "importance": n.importance,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&nodes)?);
    } else {
        println!("Total: {} nodes", resp.total_count);
        print_node_table(&resp.nodes);
    }

    Ok(())
}

async fn delete(args: NodeDeleteArgs, server: &str) -> Result<()> {
    if !args.yes {
        use inquire::Confirm;
        let confirmed = Confirm::new(&format!("Delete node {}?", args.id))
            .with_default(false)
            .prompt()?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let mut client = grpc_connect(server).await?;
    let resp = client
        .delete_node(DeleteNodeRequest {
            id: args.id.clone(),
        })
        .await?
        .into_inner();

    if resp.success {
        println!("Deleted node {}", args.id);
    } else {
        println!("Node {} not found", args.id);
    }

    Ok(())
}

async fn stats(args: NodeStatsArgs, server: &str) -> Result<()> {
    use cortex_proto::GetNodeRequest;

    let mut client = grpc_connect(server).await?;
    let n = client
        .get_node(GetNodeRequest { id: args.id })
        .await?
        .into_inner();

    if args.format == "json" {
        println!(
            "{}",
            serde_json::json!({
                "id": n.id,
                "kind": n.kind,
                "title": n.title,
                "access_count": n.access_count,
                "last_accessed_at": fmt_timestamp(n.last_accessed_at.as_ref()),
                "created_at": fmt_timestamp(n.created_at.as_ref()),
                "updated_at": fmt_timestamp(n.updated_at.as_ref()),
                "days_since_access": days_since(n.last_accessed_at.as_ref()),
            })
        );
    } else {
        println!();
        println!("Node Access Stats");
        println!("{}", "─".repeat(50));
        println!("ID:               {}", n.id);
        println!("Kind:             {}", n.kind);
        println!("Title:            {}", crate::cli::truncate(&n.title, 40));
        println!("{}", "─".repeat(50));
        println!("Access count:     {}", n.access_count);
        println!(
            "Last accessed:    {}",
            fmt_timestamp(n.last_accessed_at.as_ref())
        );
        if let Some(days) = days_since(n.last_accessed_at.as_ref()) {
            println!("Days idle:        {:.1}", days);
        }
        println!("Created:          {}", fmt_timestamp(n.created_at.as_ref()));
        println!("Updated:          {}", fmt_timestamp(n.updated_at.as_ref()));
        println!("{}", "─".repeat(50));
        println!();
    }

    Ok(())
}

/// Format an optional protobuf Timestamp as a human-readable UTC string.
fn fmt_timestamp(ts: Option<&prost_types::Timestamp>) -> String {
    match ts {
        None => "—".to_string(),
        Some(t) => {
            match chrono::DateTime::from_timestamp(t.seconds, t.nanos as u32) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
                None => "invalid".to_string(),
            }
        }
    }
}

/// Return fractional days since the given timestamp, or None if unavailable.
fn days_since(ts: Option<&prost_types::Timestamp>) -> Option<f64> {
    let t = ts?;
    let dt = chrono::DateTime::from_timestamp(t.seconds, t.nanos as u32)?;
    let elapsed = chrono::Utc::now().signed_duration_since(dt);
    Some(elapsed.num_seconds().max(0) as f64 / 86_400.0)
}

pub fn print_node_detail(n: &NodeResponse) {
    println!("ID:         {}", n.id);
    println!("Kind:       {}", n.kind);
    println!("Title:      {}", n.title);
    println!("Body:       {}", crate::cli::truncate(&n.body, 120));
    println!("Importance: {:.2}", n.importance);
    println!("Tags:       {}", n.tags.join(", "));
    println!("Source:     {}", n.source_agent);
    println!("Access:     {}", n.access_count);
    println!(
        "Last seen:  {}",
        fmt_timestamp(n.last_accessed_at.as_ref())
    );
    println!("Embedding:  {}", if n.has_embedding { "yes" } else { "no" });
}
