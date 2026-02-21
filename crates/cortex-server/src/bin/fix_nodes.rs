/// One-shot node schema repair.
///
/// Deserializes every node with the old `Node` layout (missing `last_accessed_at`),
/// sets `last_accessed_at = DateTime::UNIX_EPOCH`, and re-serializes with the new layout.
///
/// Usage: fix_nodes [path-to-cortex.redb]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use cortex_core::{Embedding, NodeKind, Source};
use redb::{Database, ReadableTable, TableDefinition};

const NODES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("nodes");

/// Node layout before `last_accessed_at` was added (same field order as the old struct).
#[derive(Serialize, Deserialize, Debug)]
struct NodeV1 {
    id: Uuid,
    kind: NodeKind,
    data: NodeDataV1,
    embedding: Option<Embedding>,
    source: Source,
    importance: f32,
    access_count: u64,
    // NOTE: no last_accessed_at
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct NodeDataV1 {
    title: String,
    body: String,
    metadata: HashMap<String, serde_json::Value>,
    tags: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/home/mike/.cortex/data/cortex.redb".to_string());
    let db_path = std::path::PathBuf::from(&db_path);

    if !db_path.exists() {
        anyhow::bail!("Database not found: {}", db_path.display());
    }

    // Backup first
    let backup = db_path.with_extension("redb.pre-fix.bak");
    if !backup.exists() {
        println!("Backing up {} → {}", db_path.display(), backup.display());
        std::fs::copy(&db_path, &backup)?;
        println!("Backup written ({} bytes)", std::fs::metadata(&backup)?.len());
    } else {
        println!("Backup already exists at {}, skipping.", backup.display());
    }

    // Open DB with repair callback
    let db = Database::builder()
        .set_repair_callback(|_| {})
        .open(&db_path)?;

    // Collect all raw node bytes
    let mut raw_nodes: Vec<([u8; 16], Vec<u8>)> = Vec::new();
    {
        let rtxn = db.begin_read()?;
        let table = rtxn.open_table(NODES)?;
        for item in table.iter()? {
            let (k, v) = item?;
            raw_nodes.push((*k.value(), v.value().to_vec()));
        }
    }

    println!("Found {} node records", raw_nodes.len());

    let mut migrated = 0u64;
    let mut already_ok = 0u64;
    let mut failed = 0u64;

    let wtxn = db.begin_write()?;
    {
        let mut table = wtxn.open_table(NODES)?;

        for (key, bytes) in &raw_nodes {
            // Try new format first
            if let Ok(new_node) = bincode::deserialize::<cortex_core::Node>(bytes) {
                // Already deserializes fine with new layout — skip
                let _ = new_node;
                already_ok += 1;
                continue;
            }

            // Try old format (without last_accessed_at)
            match bincode::deserialize::<NodeV1>(bytes) {
                Ok(old) => {
                    // Reconstruct as new Node
                    let new_node = cortex_core::Node {
                        id: old.id,
                        kind: old.kind,
                        data: cortex_core::NodeData {
                            title: old.data.title,
                            body: old.data.body,
                            metadata: old.data.metadata,
                            tags: old.data.tags,
                        },
                        embedding: old.embedding,
                        source: old.source,
                        importance: old.importance,
                        access_count: old.access_count,
                        last_accessed_at: DateTime::<Utc>::UNIX_EPOCH,
                        created_at: old.created_at,
                        updated_at: old.updated_at,
                        deleted: old.deleted,
                    };

                    let new_bytes = bincode::serialize(&new_node)?;
                    table.insert(key, new_bytes.as_slice())?;
                    migrated += 1;
                }
                Err(e) => {
                    eprintln!(
                        "  WARN: could not deserialize node {:?} as v1 either: {}",
                        Uuid::from_bytes(*key),
                        e
                    );
                    failed += 1;
                }
            }
        }
    }
    wtxn.commit()?;

    println!("\nResults:");
    println!("  {} nodes already in new format", already_ok);
    println!("  {} nodes migrated from old format", migrated);
    println!("  {} nodes could not be recovered", failed);
    println!("\nDone. Start cortex normally.");

    Ok(())
}
