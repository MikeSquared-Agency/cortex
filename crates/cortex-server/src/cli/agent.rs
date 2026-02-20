use super::{
    AgentBindArgs, AgentCommands, AgentHistoryArgs, AgentListArgs, AgentObserveArgs,
    AgentResolveArgs, AgentSelectArgs, AgentShowArgs, AgentUnbindArgs,
};
use anyhow::Result;

/// Derive the HTTP base URL from the gRPC server address by swapping the port.
/// The gRPC addr defaults to :9090 and HTTP to :9091.
fn http_base(server: &str) -> String {
    // server is like "http://localhost:9090" — replace the gRPC port with HTTP port
    if let Some(stripped) = server.strip_suffix(":9090") {
        format!("{}:9091", stripped)
    } else {
        // Best-effort: assume the HTTP server is on port 9091 of the same host
        let host = server
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("localhost");
        format!("http://{}:9091", host)
    }
}

pub async fn run(cmd: AgentCommands, server: &str) -> Result<()> {
    let base = http_base(server);
    match cmd {
        AgentCommands::List(args) => list(args, &base).await,
        AgentCommands::Show(args) => show(args, &base).await,
        AgentCommands::Bind(args) => bind(args, &base).await,
        AgentCommands::Unbind(args) => unbind(args, &base).await,
        AgentCommands::Resolve(args) => resolve(args, &base).await,
        AgentCommands::Select(args) => select(args, &base).await,
        AgentCommands::History(args) => history(args, &base).await,
        AgentCommands::Observe(args) => observe(args, &base).await,
    }
}

async fn list(_args: AgentListArgs, base: &str) -> Result<()> {
    // List all nodes of kind=agent via the nodes API
    let client = reqwest::Client::new();
    let url = format!("{}/nodes?kind=agent&limit=100", base);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    let body: serde_json::Value = resp.json().await?;
    let nodes = body["data"].as_array().cloned().unwrap_or_default();

    if nodes.is_empty() {
        println!("(no agents found — create one via `cortex node create --kind agent --title <name>`)");
        return Ok(());
    }

    println!("{:<36}  {:<20}  {}", "ID", "NAME", "TAGS");
    println!("{}", "─".repeat(70));
    for node in &nodes {
        let id = node["id"].as_str().unwrap_or("-");
        let title = node["title"].as_str().unwrap_or("-");
        let tags: Vec<&str> = node["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        println!("{:<36}  {:<20}  {}", id, title, tags.join(", "));
    }

    Ok(())
}

async fn show(args: AgentShowArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/prompts", base, args.name);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let bindings = body["data"].as_array().cloned().unwrap_or_default();

    if bindings.is_empty() {
        println!("Agent '{}' has no prompt bindings.", args.name);
        return Ok(());
    }

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&bindings)?);
        return Ok(());
    }

    println!("Prompts bound to agent '{}':", args.name);
    println!("{:<6}  {:<30}  {:<36}  {}", "WEIGHT", "SLUG", "NODE ID", "EDGE ID");
    println!("{}", "─".repeat(90));
    for b in &bindings {
        println!(
            "{:<6.2}  {:<30}  {:<36}  {}",
            b["weight"].as_f64().unwrap_or(0.0),
            b["slug"].as_str().unwrap_or("-"),
            b["id"].as_str().unwrap_or("-"),
            b["edge_id"].as_str().unwrap_or("-"),
        );
    }

    Ok(())
}

async fn bind(args: AgentBindArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/prompts/{}", base, args.name, args.slug);
    let resp = client
        .put(&url)
        .json(&serde_json::json!({"weight": args.weight}))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(data)?);
    } else {
        println!(
            "Bound prompt '{}' → agent '{}' (weight: {:.2}, edge: {})",
            args.slug,
            args.name,
            data["weight"].as_f64().unwrap_or(f64::from(args.weight)),
            data["edge_id"].as_str().unwrap_or("-"),
        );
    }

    Ok(())
}

async fn unbind(args: AgentUnbindArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/prompts/{}", base, args.name, args.slug);
    let resp = client
        .delete(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    println!("Unbound prompt '{}' from agent '{}'.", args.slug, args.name);

    Ok(())
}

async fn resolve(args: AgentResolveArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/resolved-prompt", base, args.name);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        _ => {
            let agent = data["agent"].as_str().unwrap_or(&args.name);
            let count = data["prompts_consulted"].as_u64().unwrap_or(0);
            eprintln!("# Resolved prompt for {} ({} prompt(s))", agent, count);
            eprintln!();
            println!("{}", data["resolved"].as_str().unwrap_or("(empty)"));
        }
    }

    Ok(())
}

async fn select(args: AgentSelectArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/agents/{}/active-variant?sentiment={}&task_type={}&correction_rate={}&topic_shift={}&energy={}&epsilon={}",
        base,
        args.name,
        args.sentiment,
        args.task_type,
        args.correction_rate,
        args.topic_shift,
        args.energy,
        args.epsilon,
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    }

    if let Some(sel) = data["selected"].as_object() {
        let slug = sel["slug"].as_str().unwrap_or("-");
        let total = sel["total_score"].as_f64().unwrap_or(0.0);
        let edge_w = sel["edge_weight"].as_f64().unwrap_or(0.0);
        let ctx = sel["context_score"].as_f64().unwrap_or(0.0);
        let swap = data["swap_recommended"].as_bool().unwrap_or(false);

        println!("Selected variant for agent '{}':", args.name);
        println!("  Slug:          {}", slug);
        println!("  Total score:   {:.3}", total);
        println!("  Edge weight:   {:.3}", edge_w);
        println!("  Context score: {:.3}", ctx);
        if swap {
            println!("  ⚡ Swap recommended (differs from current active variant)");
        }

        if let Some(all) = data["all_variants"].as_array() {
            if all.len() > 1 {
                println!();
                println!("{:<30}  {:<8}  {:<8}  {}", "SLUG", "EDGE", "CTX", "TOTAL");
                println!("{}", "─".repeat(60));
                for v in all {
                    println!(
                        "{:<30}  {:<8.3}  {:<8.3}  {:.3}",
                        v["slug"].as_str().unwrap_or("-"),
                        v["edge_weight"].as_f64().unwrap_or(0.0),
                        v["context_score"].as_f64().unwrap_or(0.0),
                        v["total_score"].as_f64().unwrap_or(0.0),
                    );
                }
            }
        }
    } else {
        println!("Agent '{}' has no prompt variants bound.", args.name);
    }

    Ok(())
}

async fn history(args: AgentHistoryArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/variant-history?limit={}", base, args.name, args.limit);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let items = body["data"].as_array().cloned().unwrap_or_default();

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if items.is_empty() {
        println!("No variant history for agent '{}'.", args.name);
        return Ok(());
    }

    println!("{:<10}  {:<30}  {:<8}  {:<10}  {}", "TYPE", "VARIANT", "SCORE", "OUTCOME", "TIMESTAMP");
    println!("{}", "─".repeat(80));
    for item in &items {
        let obs_type = item["type"].as_str().unwrap_or("?");
        let slug = item["variant_slug"]
            .as_str()
            .or_else(|| item["new_variant_slug"].as_str())
            .unwrap_or("-");
        let score = item["observation_score"]
            .as_f64()
            .map(|s| format!("{:.3}", s))
            .unwrap_or_else(|| "-".into());
        let outcome = item["task_outcome"].as_str().unwrap_or("-");
        let ts = item["created_at"].as_str().unwrap_or("-");
        println!("{:<10}  {:<30}  {:<8}  {:<10}  {}", obs_type, slug, score, outcome, ts);
    }

    Ok(())
}

async fn observe(args: AgentObserveArgs, base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/agents/{}/observe", base, args.name);

    let mut payload = serde_json::json!({
        "variant_id": args.variant_id,
        "variant_slug": args.variant_slug,
        "sentiment_score": args.sentiment_score,
        "correction_count": args.correction_count,
        "task_outcome": args.task_outcome,
    });
    if let Some(tc) = args.token_cost {
        payload["token_cost"] = serde_json::json!(tc);
    }

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        let err = body["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("{}", err);
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    println!("Observation recorded for agent '{}':", args.name);
    println!("  Observation ID:   {}", data["observation_id"].as_str().unwrap_or("-"));
    println!("  Variant:          {}", data["variant_slug"].as_str().unwrap_or("-"));
    println!("  Score:            {:.3}", data["observation_score"].as_f64().unwrap_or(0.0));
    println!(
        "  Edge weight:      {:.3} → {:.3}",
        data["old_edge_weight"].as_f64().unwrap_or(0.0),
        data["new_edge_weight"].as_f64().unwrap_or(0.0),
    );

    Ok(())
}
