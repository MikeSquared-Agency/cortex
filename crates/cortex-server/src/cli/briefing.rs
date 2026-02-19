use anyhow::Result;
use cortex_proto::BriefingRequest;
use crate::cli::{BriefingArgs, grpc_connect};

pub async fn run(args: BriefingArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    let resp = client
        .get_briefing(BriefingRequest {
            agent_id: args.agent_id,
            compact: args.compact,
        })
        .await?
        .into_inner();

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::json!({
                "agent_id": resp.agent_id,
                "rendered": resp.rendered,
                "generated_at": resp.generated_at,
                "nodes_consulted": resp.nodes_consulted,
                "cached": resp.cached,
            }));
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
