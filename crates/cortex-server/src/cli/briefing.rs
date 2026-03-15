use crate::cli::{grpc_connect, BriefingArgs};
use anyhow::Result;
use cortex_proto::BriefingRequest;

pub async fn run(args: BriefingArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    // Determine scope and agent_ids from CLI args
    let (scope, agent_id, agent_ids) = if let Some(ref agents_str) = args.agents {
        let ids: Vec<String> = agents_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let primary = ids.first().cloned().unwrap_or_default();
        ("unified".to_string(), primary, ids)
    } else {
        (args.scope.clone(), args.agent_id.clone(), vec![])
    };

    let resp = client
        .get_briefing(BriefingRequest {
            agent_id,
            compact: args.compact,
            scope,
            agent_ids,
        })
        .await?
        .into_inner();

    match args.format.as_str() {
        "json" => {
            println!(
                "{}",
                serde_json::json!({
                    "agent_id": resp.agent_id,
                    "rendered": resp.rendered,
                    "generated_at": resp.generated_at,
                    "nodes_consulted": resp.nodes_consulted,
                    "cached": resp.cached,
                })
            );
        }
        _ => {
            if resp.cached {
                eprintln!("(cached, generated at {})", resp.generated_at);
            }
            println!("{}", resp.rendered);
        }
    }

    Ok(())
}
