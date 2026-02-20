use super::{
    PromptCommands, PromptDeployArgs, PromptGetArgs, PromptListArgs, PromptMigrateArgs,
    PromptPerformanceArgs, PromptRollbackStatusArgs, PromptUnquarantineArgs,
};
use crate::config::CortexConfig;
use anyhow::Result;
use cortex_core::prompt::{PromptContent, PromptResolver};
use cortex_core::relations::defaults::inherits_from;
use cortex_core::{Edge, EdgeProvenance, RedbStorage, Storage};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn run(cmd: PromptCommands, config: &CortexConfig, server: &str) -> Result<()> {
    match cmd {
        PromptCommands::List(args) => list(args, config).await,
        PromptCommands::Get(args) => get(args, config).await,
        PromptCommands::Migrate(args) => migrate(args, config).await,
        PromptCommands::Performance(args) => performance(args, server).await,
        PromptCommands::Deploy(args) => deploy(args, server).await,
        PromptCommands::RollbackStatus(args) => rollback_status(args, server).await,
        PromptCommands::Unquarantine(args) => unquarantine(args, server).await,
    }
}

fn open_storage(config: &CortexConfig) -> Result<Arc<RedbStorage>> {
    let db_path = config.db_path();
    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {:?}. Run `cortex init` or `cortex serve` first.",
            db_path
        );
    }
    Ok(Arc::new(RedbStorage::open(&db_path)?))
}

async fn list(args: PromptListArgs, config: &CortexConfig) -> Result<()> {
    let storage = open_storage(config)?;
    let resolver = PromptResolver::new(storage);
    let mut prompts = resolver.list_all_prompts()?;

    if let Some(branch) = &args.branch {
        prompts.retain(|p| &p.branch == branch);
    }

    if prompts.is_empty() {
        println!("(no prompts found)");
        return Ok(());
    }

    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&prompts)?),
        _ => {
            println!("{:<30}  {:<12}  {:<14}  {:<5}  {}", "SLUG", "TYPE", "BRANCH", "VER", "NODE ID");
            println!("{}", "─".repeat(100));
            for p in &prompts {
                println!(
                    "{:<30}  {:<12}  {:<14}  {:<5}  {}",
                    super::truncate(&p.slug, 30),
                    super::truncate(&p.prompt_type, 12),
                    super::truncate(&p.branch, 14),
                    p.version,
                    p.node_id,
                );
            }
            println!();
            println!("{} prompt(s)", prompts.len());
        }
    }

    Ok(())
}

async fn get(args: PromptGetArgs, config: &CortexConfig) -> Result<()> {
    let storage = open_storage(config)?;
    let resolver = PromptResolver::new(storage);
    let branch = args.branch.as_deref().unwrap_or("main");

    if let Some(version_num) = args.version {
        let node = resolver
            .get_version(&args.slug, branch, version_num)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Prompt '{}@{}/v{}' not found",
                    args.slug,
                    branch,
                    version_num
                )
            })?;
        let content = resolver.parse_content(&node)?;
        match args.format.as_str() {
            "json" => println!("{}", serde_json::to_string_pretty(&content)?),
            _ => print_raw_content(&args.slug, branch, version_num, &content),
        }
    } else {
        let node = resolver
            .find_head(&args.slug, branch)?
            .ok_or_else(|| {
                anyhow::anyhow!("Prompt '{}@{}' not found", args.slug, branch)
            })?;
        let resolved = resolver.resolve(&node)?;
        match args.format.as_str() {
            "json" => println!("{}", serde_json::to_string_pretty(&resolved)?),
            _ => {
                println!("Prompt: {}@{}/v{}", resolved.slug, resolved.branch, resolved.version);
                println!("Type:   {}", resolved.prompt_type);
                println!("Node:   {}", resolved.node_id);
                if !resolved.lineage.is_empty() {
                    println!("Lineage: {}", resolved.lineage.join(" → "));
                }
                if !resolved.skills.is_empty() {
                    println!("Skills: {}", resolved.skills.join(", "));
                }
                println!();
                println!("Resolved sections:");
                for (k, v) in &resolved.content {
                    println!("  [{}]", k);
                    println!("  {}", serde_json::to_string_pretty(v).unwrap_or_default());
                }
            }
        }
    }

    Ok(())
}

fn print_raw_content(slug: &str, branch: &str, version: u32, content: &PromptContent) {
    println!("Prompt: {}@{}/v{}", slug, branch, version);
    println!("Type:   {}", content.prompt_type);
    println!();
    println!("Sections:");
    for (k, v) in &content.sections {
        println!("  [{}]", k);
        println!("  {}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
    if !content.override_sections.is_empty() {
        println!();
        println!("Override sections:");
        for (k, v) in &content.override_sections {
            println!("  [{}]", k);
            println!("  {}", serde_json::to_string_pretty(v).unwrap_or_default());
        }
    }
}

// ── Performance ─────────────────────────────────────────────────────────────

/// Derive the HTTP base URL from the gRPC server address.
fn http_base(server: &str) -> String {
    if let Some(stripped) = server.strip_suffix(":9090") {
        format!("{}:9091", stripped)
    } else {
        let host = server
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("localhost");
        format!("http://{}:9091", host)
    }
}

async fn performance(args: PromptPerformanceArgs, server: &str) -> Result<()> {
    let base = http_base(server);
    let client = reqwest::Client::new();
    let url = format!("{}/prompts/{}/performance?limit={}", base, args.slug, args.limit);
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

    println!("Performance for prompt '{}':", args.slug);
    println!("  Observations: {}", data["observation_count"].as_u64().unwrap_or(0));
    println!("  Avg score:    {:.3}", data["avg_score"].as_f64().unwrap_or(0.0));
    println!("  Avg sentiment:{:.3}", data["avg_sentiment"].as_f64().unwrap_or(0.0));
    println!("  Avg corrs:    {:.1}", data["avg_correction_count"].as_f64().unwrap_or(0.0));

    if let Some(outcomes) = data["task_outcomes"].as_object() {
        println!("  Outcomes:");
        let mut sorted: Vec<_> = outcomes.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());
        for (outcome, count) in sorted {
            println!("    {}: {}", outcome, count);
        }
    }

    if let Some(obs) = data["observations"].as_array() {
        if !obs.is_empty() {
            println!();
            println!("{:<8}  {:<10}  {:<8}  {}", "SCORE", "OUTCOME", "CORRS", "TIMESTAMP");
            println!("{}", "─".repeat(55));
            for o in obs {
                println!(
                    "{:<8.3}  {:<10}  {:<8}  {}",
                    o["observation_score"].as_f64().unwrap_or(0.0),
                    o["task_outcome"].as_str().unwrap_or("-"),
                    o["correction_count"].as_u64().unwrap_or(0),
                    o["created_at"].as_str().unwrap_or("-"),
                );
            }
        }
    }

    Ok(())
}

// ── Deploy ───────────────────────────────────────────────────────────────────

async fn deploy(args: PromptDeployArgs, server: &str) -> Result<()> {
    let base = http_base(server);
    let client = reqwest::Client::new();
    let url = format!("{}/prompts/{}/deploy", base, args.slug);
    let payload = serde_json::json!({
        "branch": args.branch,
        "agent_name": args.agent_name,
        "baseline_sample_size": args.baseline_sample_size,
    });
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        anyhow::bail!("{}", body["error"].as_str().unwrap_or("unknown error"));
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    }

    println!("Deployment recorded for '{}'@{}/v{}:", args.slug, args.branch, data["version"].as_u64().unwrap_or(0));
    println!("  Deployment node: {}", data["deployment_node_id"].as_str().unwrap_or("-"));
    println!("  Prompt node:     {}", data["prompt_node_id"].as_str().unwrap_or("-"));
    println!("  Baseline corr:   {:.3}", data["baseline_correction_rate"].as_f64().unwrap_or(0.0));
    println!("  Baseline senti:  {:.3}", data["baseline_sentiment"].as_f64().unwrap_or(0.0));
    println!("  Baseline sample: {}", data["baseline_sample_size"].as_u64().unwrap_or(0));
    println!();
    println!("Monitoring window active. Use `cortex prompt rollback-status {}` to check.", args.slug);
    Ok(())
}

// ── Rollback status ──────────────────────────────────────────────────────────

async fn rollback_status(args: PromptRollbackStatusArgs, server: &str) -> Result<()> {
    let base = http_base(server);
    let client = reqwest::Client::new();
    let url = format!("{}/prompts/{}/rollback-status?branch={}", base, args.slug, args.branch);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        anyhow::bail!("{}", body["error"].as_str().unwrap_or("unknown error"));
    }

    let body: serde_json::Value = resp.json().await?;
    let data = &body["data"];

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    }

    let quarantined = data["is_quarantined"].as_bool().unwrap_or(false);
    println!("Rollback status for '{}'@{}:", args.slug, args.branch);
    println!("  Current version: v{}", data["current_version"].as_u64().unwrap_or(0));
    println!("  Quarantined:     {}", if quarantined { "YES" } else { "no" });
    println!("  Rollback count:  {}", data["rollback_count"].as_u64().unwrap_or(0));
    if let Some(expires) = data["cooldown_expires_at"].as_str() {
        println!("  Cooldown until:  {}", expires);
    }

    if let Some(dep) = data["active_deployment"].as_object() {
        println!();
        println!("  Active monitoring window:");
        println!("    Observations: {}/{}", dep["n_observed"].as_u64().unwrap_or(0), dep["monitoring_window"].as_u64().unwrap_or(0));
        println!("    Agent:        {}", dep["agent_name"].as_str().unwrap_or("-"));
        println!("    Deployed at:  {}", dep["deployed_at"].as_str().unwrap_or("-"));
        println!("    Mean corr:    {:.3}  (baseline {:.3})", dep["mean_correction"].as_f64().unwrap_or(0.0), dep["baseline_correction_rate"].as_f64().unwrap_or(0.0));
        println!("    Mean senti:   {:.3}  (baseline {:.3})", dep["mean_sentiment"].as_f64().unwrap_or(0.0), dep["baseline_sentiment"].as_f64().unwrap_or(0.0));
        println!("    Consec neg:   {}", dep["consecutive_negative"].as_u64().unwrap_or(0));
    } else {
        println!("  Active monitoring: none");
    }

    if let Some(rollbacks) = data["recent_rollbacks"].as_array() {
        if !rollbacks.is_empty() {
            println!();
            println!("  Recent rollbacks:");
            println!("  {:<8}  {:<8}  {:<30}  {}", "FROM", "TO", "TRIGGER", "TIMESTAMP");
            println!("  {}", "─".repeat(75));
            for r in rollbacks {
                println!(
                    "  v{:<7}  v{:<7}  {:<30}  {}",
                    r["from_version"].as_u64().unwrap_or(0),
                    r["to_version"].as_u64().unwrap_or(0),
                    r["trigger"].as_str().unwrap_or("-"),
                    r["rolled_back_at"].as_str().unwrap_or("-"),
                );
            }
        }
    }

    Ok(())
}

// ── Unquarantine ─────────────────────────────────────────────────────────────

async fn unquarantine(args: PromptUnquarantineArgs, server: &str) -> Result<()> {
    let base = http_base(server);
    let client = reqwest::Client::new();
    let url = format!("{}/prompts/{}/unquarantine", base, args.slug);
    let payload = serde_json::json!({ "branch": args.branch });
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("HTTP request failed: {}. Is `cortex serve` running?", e))?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        anyhow::bail!("{}", body["error"].as_str().unwrap_or("unknown error"));
    }

    println!("Quarantine lifted for '{}'@{}.", args.slug, args.branch);
    println!("The version will now be eligible for exploration traffic (ε-greedy).");
    Ok(())
}

// ── Migration ───────────────────────────────────────────────────────────────

/// JSON structure for the migration file.
#[derive(Deserialize, Debug)]
struct MigrationFile {
    #[serde(default)]
    prompts: Vec<MigrationPrompt>,
    #[serde(default)]
    versions: Vec<MigrationVersion>,
    #[serde(default)]
    inheritance: Vec<MigrationInheritance>,
}

#[derive(Deserialize, Debug)]
struct MigrationPrompt {
    slug: String,
    name: Option<String>,
    #[serde(rename = "type")]
    prompt_type: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct MigrationVersion {
    slug: String,
    version: u32,
    branch: String,
    #[serde(default = "default_author")]
    author: String,
    content: MigrationContent,
    #[serde(default)]
    override_sections: HashMap<String, serde_json::Value>,
}

fn default_author() -> String {
    "system".to_string()
}

#[derive(Deserialize, Debug)]
struct MigrationContent {
    #[serde(rename = "type")]
    prompt_type: Option<String>,
    #[serde(default)]
    sections: HashMap<String, serde_json::Value>,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct MigrationInheritance {
    child_slug: String,
    parent_slug: String,
    #[serde(default = "default_branch")]
    child_branch: String,
    #[serde(default = "default_branch")]
    parent_branch: String,
}

fn default_branch() -> String {
    "main".to_string()
}

async fn migrate(args: PromptMigrateArgs, config: &CortexConfig) -> Result<()> {
    let raw = std::fs::read_to_string(&args.file).map_err(|e| {
        anyhow::anyhow!("Cannot read migration file {:?}: {}", args.file, e)
    })?;
    let migration: MigrationFile = serde_json::from_str(&raw).map_err(|e| {
        anyhow::anyhow!("Invalid migration JSON: {}", e)
    })?;

    if args.dry_run {
        println!(
            "Dry run: {} prompts, {} versions, {} inheritance links",
            migration.prompts.len(),
            migration.versions.len(),
            migration.inheritance.len(),
        );
        return Ok(());
    }

    let storage = open_storage(config)?;
    let resolver = PromptResolver::new(storage.clone());

    // Group versions by slug+branch, sorted by version number.
    let mut versions_by_slug: HashMap<(String, String), Vec<&MigrationVersion>> = HashMap::new();
    for v in &migration.versions {
        versions_by_slug
            .entry((v.slug.clone(), v.branch.clone()))
            .or_default()
            .push(v);
    }
    for group in versions_by_slug.values_mut() {
        group.sort_by_key(|v| v.version);
    }

    let mut created = 0usize;
    let mut skipped = 0usize;

    // Build a map from slug → MigrationPrompt for metadata lookup.
    let prompt_meta: HashMap<&str, &MigrationPrompt> =
        migration.prompts.iter().map(|p| (p.slug.as_str(), p)).collect();

    // Process all (slug, branch) groups in order.
    let mut keys: Vec<(String, String)> = versions_by_slug.keys().cloned().collect();
    keys.sort();

    for key in &keys {
        let versions = &versions_by_slug[key];
        let (slug, branch) = key;

        for mv in versions.iter() {
            let prompt_type = mv
                .content
                .prompt_type
                .clone()
                .or_else(|| {
                    prompt_meta.get(slug.as_str()).map(|p| p.prompt_type.clone())
                })
                .unwrap_or_else(|| "unknown".to_string());

            let mut metadata = mv.content.metadata.clone();
            if let Some(meta) = prompt_meta.get(slug.as_str()) {
                for (k, v) in &meta.metadata {
                    metadata.entry(k.clone()).or_insert(v.clone());
                }
                if let Some(name) = &meta.name {
                    metadata.entry("name".to_string()).or_insert_with(|| {
                        serde_json::Value::String(name.clone())
                    });
                }
            }

            let content = PromptContent {
                slug: slug.clone(),
                prompt_type,
                branch: branch.clone(),
                version: mv.version,
                sections: mv.content.sections.clone(),
                metadata,
                override_sections: mv.override_sections.clone(),
            };

            let result = if mv.version == 1 {
                // Try create_prompt for v1; skip if it already exists.
                match resolver.create_prompt(content, branch, &mv.author) {
                    Ok(_) => {
                        created += 1;
                        println!("  created {}@{}/v1", slug, branch);
                        Ok(())
                    }
                    Err(cortex_core::CortexError::Validation(msg))
                        if msg.contains("already exists") =>
                    {
                        skipped += 1;
                        println!("  skipped {}@{}/v1 (already exists)", slug, branch);
                        Ok(())
                    }
                    Err(e) => Err(anyhow::anyhow!(e)),
                }
            } else {
                // Use create_version for v2+.
                match resolver.create_version(slug, branch, content, &mv.author) {
                    Ok(node_id) => {
                        created += 1;
                        // Validate that the actual version matches what the migration expected
                        let actual_version = storage
                            .get_node(node_id)
                            .ok()
                            .flatten()
                            .and_then(|n| resolver.parse_content(&n).ok())
                            .map(|c| c.version)
                            .unwrap_or(0);
                        if actual_version != mv.version {
                            println!(
                                "  ⚠ created {}@{}/v{} (migration expected v{} — version sequence gap?)",
                                slug, branch, actual_version, mv.version,
                            );
                        } else {
                            println!("  created {}@{}/v{}", slug, branch, mv.version);
                        }
                        Ok(())
                    }
                    Err(e) => Err(anyhow::anyhow!(e)),
                }
            };

            result?;
        }
    }

    // Link inheritance edges.
    println!("\nLinking {} inheritance edges...", migration.inheritance.len());
    let mut linked = 0usize;

    for link in &migration.inheritance {
        let child_branch = link.child_branch.as_str();
        let parent_branch = link.parent_branch.as_str();

        let child = resolver.find_head(&link.child_slug, child_branch)?;
        let parent = resolver.find_head(&link.parent_slug, parent_branch)?;

        match (child, parent) {
            (Some(child_node), Some(parent_node)) => {
                // Check if edge already exists.
                let existing = storage.edges_between(child_node.id, parent_node.id)?;
                let already_linked = existing.iter().any(|e| e.relation == inherits_from());
                if !already_linked {
                    let edge = Edge::new(
                        child_node.id,
                        parent_node.id,
                        inherits_from(),
                        1.0,
                        EdgeProvenance::Imported {
                            source: "migration".to_string(),
                        },
                    );
                    storage.put_edge(&edge)?;
                    linked += 1;
                    println!("  linked {} → {}", link.child_slug, link.parent_slug);
                } else {
                    println!("  skipped {} → {} (already linked)", link.child_slug, link.parent_slug);
                }
            }
            (None, _) => println!("  skip: child '{}@{}' not found", link.child_slug, child_branch),
            (_, None) => println!("  skip: parent '{}@{}' not found", link.parent_slug, parent_branch),
        }
    }

    println!();
    println!(
        "Migration complete: {} created, {} skipped, {} inheritance edges linked",
        created, skipped, linked
    );

    Ok(())
}
