use super::AuditArgs;
use crate::config::CortexConfig;
use anyhow::Result;
use chrono::{Duration, Utc};
use cortex_core::policies::audit::AuditFilter;
use cortex_core::RedbStorage;
use std::sync::Arc;

pub async fn run(args: AuditArgs, config: CortexConfig) -> Result<()> {
    let db_path = config.db_path();
    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {:?}. Run `cortex init` or `cortex serve` first.",
            db_path
        );
    }

    let storage = RedbStorage::open(&db_path)?;
    let audit_log = Arc::new(storage.create_audit_log());

    let since = args.since.as_deref().map(parse_duration).transpose()?;
    let node_id = args
        .node
        .as_deref()
        .map(|s| uuid::Uuid::parse_str(s).map_err(|_| anyhow::anyhow!("Invalid UUID: {}", s)))
        .transpose()?;

    let filter = AuditFilter {
        since,
        actor: args.actor.clone(),
        node_id,
        action: None,
        limit: Some(args.limit),
    };

    let entries = audit_log.query(filter)?;

    if entries.is_empty() {
        println!("(no audit entries found)");
        return Ok(());
    }

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
        _ => {
            println!(
                "{:<24}  {:<20}  {:<36}  {}",
                "TIME", "ACTION", "TARGET", "ACTOR"
            );
            println!("{}", "─".repeat(90));
            for entry in &entries {
                println!(
                    "{:<24}  {:<20}  {:<36}  {}",
                    entry.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
                    entry.action.to_string(),
                    entry.target_id,
                    entry.actor,
                );
                if let Some(ref details) = entry.details {
                    println!("  → {}", details);
                }
            }
            println!();
            println!("{} entries", entries.len());
        }
    }

    Ok(())
}

/// Parse a human-readable duration like "24h", "7d", "1h30m" into a UTC timestamp.
fn parse_duration(s: &str) -> Result<chrono::DateTime<Utc>> {
    let s = s.trim();
    let mut remaining = s;
    let mut total_seconds: i64 = 0;

    while !remaining.is_empty() {
        let split_at = remaining
            .find(|c: char| c.is_alphabetic())
            .ok_or_else(|| anyhow::anyhow!("Cannot parse duration '{}': expected format like '24h', '7d', '1h30m'", s))?;

        let num: i64 = remaining[..split_at]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid number in duration '{}'", s))?;

        let rest = &remaining[split_at..];
        let unit_end = rest.find(|c: char| c.is_ascii_digit()).unwrap_or(rest.len());
        let unit = &rest[..unit_end];

        let secs = match unit {
            "s" => num,
            "m" => num * 60,
            "h" => num * 3600,
            "d" => num * 86400,
            "w" => num * 7 * 86400,
            _ => anyhow::bail!("Unknown time unit '{}' in duration '{}'", unit, s),
        };
        total_seconds += secs;
        remaining = &rest[unit_end..];
    }

    Ok(Utc::now() - Duration::seconds(total_seconds))
}
