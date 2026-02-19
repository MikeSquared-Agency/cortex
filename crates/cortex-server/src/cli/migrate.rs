use crate::config::CortexConfig;
use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const NODES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("nodes");

pub async fn run(config: CortexConfig) -> Result<()> {
    let db_path = config.db_path();

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cortex init` first.",
            db_path.display()
        );
    }

    println!("Cortex data at {}", db_path.display());

    let current_version = read_schema_version(&db_path)?;
    let target_version = cortex_core::CURRENT_SCHEMA_VERSION;

    println!("Current schema: v{}", current_version);
    println!("Target schema:  v{}", target_version);

    if current_version == target_version {
        println!("✅ Already up to date.");
        return Ok(());
    }

    if current_version > target_version {
        anyhow::bail!(
            "Database schema v{} is newer than this binary (v{}). Upgrade Cortex.",
            current_version,
            target_version
        );
    }

    // Create backup before migration
    let backup_path = db_path.with_extension(format!("redb.v{}.bak", current_version));
    println!("\nCreating backup at {}...", backup_path.display());
    std::fs::copy(&db_path, &backup_path)?;
    println!("done");

    // Apply migrations
    for from in current_version..target_version {
        let to = from + 1;
        let start = std::time::Instant::now();
        print!("Applying v{} → v{}...", from, to);
        apply_migration(&db_path, from, to)?;
        println!(" done ({:.1}s)", start.elapsed().as_secs_f32());
    }

    println!("\n✅ Schema upgraded to v{}.", target_version);
    Ok(())
}

fn read_schema_version(path: &std::path::Path) -> Result<u32> {
    let db = Database::create(path)?;
    let read_txn = db.begin_read()?;

    let version = match read_txn.open_table(META) {
        Ok(table) => table
            .get("schema_version")
            .ok()
            .flatten()
            .and_then(|v| {
                std::str::from_utf8(v.value())
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .unwrap_or(1),
        Err(_) => 1,
    };

    Ok(version)
}

fn apply_migration(path: &std::path::Path, from: u32, to: u32) -> Result<()> {
    match (from, to) {
        (1, 2) => migrate_v1_to_v2(path),
        (f, t) => anyhow::bail!("No migration path from v{} to v{}", f, t),
    }
}

fn migrate_v1_to_v2(path: &std::path::Path) -> Result<()> {
    // v1 → v2: NodeKind changed from u8 discriminant to UTF-8 string.
    //
    // If the DB was created fresh with v2 code, all nodes already deserialize correctly.
    // We just verify a sample node, then write the new schema version.
    //
    // If genuine v1 (enum-encoded) data exists, deserialization will fail.
    // In that case, the user must export with the v1 binary and re-import.

    let db = Database::create(path)?;

    // Sample the first node to check if deserialization works
    let read_txn = db.begin_read()?;
    let nodes_readable = read_txn.open_table(NODES);

    if let Ok(table) = nodes_readable {
        let mut iter = table.iter()?;
        if let Some(entry) = iter.next() {
            let entry = entry?;
            let bytes = entry.1.value();
            if cortex_core::storage::RedbStorage::try_deserialize_node(bytes).is_err() {
                anyhow::bail!(
                    "Database contains genuine v1 (enum-encoded) NodeKind data.\n\
                     To migrate: export data with the v1 binary, then re-import:\n\
                     \n  cortex export --format jsonl > backup.jsonl  (with v1 binary)\n\
                     \n  cortex import backup.jsonl  (with v2 binary)"
                );
            }
        }
    }

    drop(read_txn);

    // Update schema version to v2
    let write_txn = db.begin_write()?;
    {
        let mut meta = write_txn.open_table(META)?;
        meta.insert("schema_version", "2".as_bytes())?;
    }
    write_txn.commit()?;

    Ok(())
}
