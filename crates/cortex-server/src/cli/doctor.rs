use crate::config::CortexConfig;
use anyhow::Result;
use cortex_core::{NodeFilter, RedbStorage, Storage};

#[derive(Debug)]
enum CheckStatus {
    Ok,
    Warning,
    Error,
}

struct CheckResult {
    name: String,
    status: CheckStatus,
    detail: String,
    fix_hint: Option<String>,
}

pub async fn run(config: CortexConfig, _server: &str) -> Result<()> {
    println!();
    println!("Cortex Health Check");
    println!("{}", "─".repeat(50));

    let mut results = Vec::new();

    let db_path = config.db_path();

    // Check 1: DB file accessible
    results.push(if db_path.exists() {
        CheckResult {
            name: "Database file accessible".into(),
            status: CheckStatus::Ok,
            detail: db_path.display().to_string(),
            fix_hint: None,
        }
    } else {
        CheckResult {
            name: "Database file accessible".into(),
            status: CheckStatus::Error,
            detail: format!("{} not found", db_path.display()),
            fix_hint: Some("Run `cortex init` to create a new database".into()),
        }
    });

    // Check 2: Schema version
    let storage = if db_path.exists() {
        match RedbStorage::open(&db_path) {
            Ok(s) => {
                results.push(CheckResult {
                    name: "Schema version".into(),
                    status: CheckStatus::Ok,
                    detail: format!("v{} (current)", cortex_core::CURRENT_SCHEMA_VERSION),
                    fix_hint: None,
                });
                Some(s)
            }
            Err(e) => {
                let hint = if e.to_string().contains("older") {
                    Some("Run `cortex migrate` to upgrade the schema".into())
                } else {
                    None
                };
                results.push(CheckResult {
                    name: "Schema version".into(),
                    status: CheckStatus::Error,
                    detail: e.to_string(),
                    fix_hint: hint,
                });
                None
            }
        }
    } else {
        None
    };

    if let Some(ref storage) = storage {
        // Check 3: Node/edge counts
        let stats = storage.stats()?;

        // Check 4: Orphaned edges (edges where from/to node doesn't exist)
        let all_nodes = storage.list_nodes(NodeFilter::new().include_deleted())?;
        let node_ids: std::collections::HashSet<_> = all_nodes.iter().map(|n| n.id).collect();

        let mut orphaned_edge_count = 0;
        for node in &all_nodes {
            let edges_from = storage.edges_from(node.id)?;
            for edge in &edges_from {
                if !node_ids.contains(&edge.to) {
                    orphaned_edge_count += 1;
                }
            }
        }

        results.push(if orphaned_edge_count == 0 {
            CheckResult {
                name: "Orphaned edges".into(),
                status: CheckStatus::Ok,
                detail: "None found".into(),
                fix_hint: None,
            }
        } else {
            CheckResult {
                name: "Orphaned edges".into(),
                status: CheckStatus::Error,
                detail: format!("{} edges reference non-existent nodes", orphaned_edge_count),
                fix_hint: Some("Run `cortex doctor --fix` to prune orphaned edges".into()),
            }
        });

        // Check 5: Missing embeddings
        let missing_embeddings = all_nodes
            .iter()
            .filter(|n| !n.deleted && n.embedding.is_none())
            .count();

        results.push(if missing_embeddings == 0 {
            CheckResult {
                name: "Embedding coverage".into(),
                status: CheckStatus::Ok,
                detail: format!("{} nodes with embeddings", stats.node_count),
                fix_hint: None,
            }
        } else {
            CheckResult {
                name: "Embedding coverage".into(),
                status: CheckStatus::Warning,
                detail: format!("{} nodes missing embeddings", missing_embeddings),
                fix_hint: Some("Run `cortex doctor --reembed` to backfill embeddings".into()),
            }
        });
    }

    // Print results
    let mut has_errors = false;
    for r in &results {
        let (symbol, _) = match r.status {
            CheckStatus::Ok => ("[✓]", false),
            CheckStatus::Warning => ("[⚠]", false),
            CheckStatus::Error => {
                has_errors = true;
                ("[✗]", true)
            }
        };
        println!("{} {}: {}", symbol, r.name, r.detail);
        if let Some(hint) = &r.fix_hint {
            println!("    → {}", hint);
        }
    }

    println!("{}", "─".repeat(50));

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}
