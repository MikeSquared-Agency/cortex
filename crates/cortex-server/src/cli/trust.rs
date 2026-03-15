use crate::cli::TrustArgs;
use crate::config::CortexConfig;
use anyhow::Result;
use cortex_core::storage::NodeFilter;
use cortex_core::{RedbStorage, Storage, TrustEngine};
use std::sync::Arc;

pub async fn run(args: TrustArgs, config: CortexConfig) -> Result<()> {
    let trust_config = config.trust.clone().unwrap_or_default();

    let db_path = config.db_path();
    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {:?}. Run `cortex serve` first or check your data_dir.",
            db_path
        );
    }

    let storage = Arc::new(RedbStorage::open(&db_path)?);
    let engine = TrustEngine::new(storage.clone(), trust_config);

    if let Some(agent_id) = &args.agent {
        // Show agent reliability
        let reliability = cortex_core::trust::cache::compute_source_reliability(
            storage.as_ref(),
            agent_id,
        )?;

        let node_count = storage.count_nodes(
            NodeFilter::new().with_source_agent(agent_id.to_string()),
        )?;

        match args.format.as_str() {
            "json" => {
                println!(
                    "{}",
                    serde_json::json!({
                        "agent": agent_id,
                        "reliability": reliability,
                        "total_nodes": node_count,
                    })
                );
            }
            _ => {
                println!("Agent: {}", agent_id);
                println!("Reliability: {:.3}", reliability);
                println!("Total nodes: {}", node_count);
            }
        }
        return Ok(());
    }

    if let Some(node_id_str) = &args.id {
        let id: uuid::Uuid = node_id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid node ID: {}", node_id_str))?;

        let node = storage
            .get_node(id)?
            .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node_id_str))?;

        let score = engine.score(&node)?;

        match args.format.as_str() {
            "json" => {
                println!("{}", serde_json::to_string_pretty(&score)?);
            }
            _ => {
                println!("Node:    {} ({})", node.data.title, node.id);
                println!("Kind:    {}", node.kind);
                println!("Agent:   {}", node.source.agent);
                println!("{}", "─".repeat(50));
                println!("Trust:            {:.3}", score.total);
                println!("  Corroboration:  {:.3}", score.corroboration);
                println!("  Contradiction:  {:.3}  ({} conflicts)", score.contradiction, score.contradiction_count);
                println!("  Source:         {:.3}", score.source_reliability);
                println!("  Access:         {:.3}", score.access);
                println!("  Freshness:      {:.3}", score.freshness);
                if !score.corroborating_agents.is_empty() {
                    println!("  Corroborated by: {}", score.corroborating_agents.join(", "));
                }
            }
        }
        return Ok(());
    }

    // No node or agent specified — show help
    eprintln!("Usage:");
    eprintln!("  cortex trust <node-id>           Show trust breakdown for a node");
    eprintln!("  cortex trust --agent <agent-id>   Show agent reliability score");
    Ok(())
}
