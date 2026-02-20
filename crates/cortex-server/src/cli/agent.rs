use super::{AgentCommands, AgentListArgs, AgentShowArgs, AgentBindArgs, AgentUnbindArgs, AgentResolveArgs};
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
