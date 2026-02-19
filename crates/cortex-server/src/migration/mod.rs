use cortex_core::storage::{RedbStorage, Storage};
use cortex_core::NodeFilter;

/// Placeholder for Alexandria migration
pub fn migrate_alexandria() -> anyhow::Result<()> {
    tracing::warn!("Alexandria migration not yet implemented");
    Ok(())
}

/// Migrate schema v1 to v2.
///
/// v1: NodeKind stored as u8 discriminant in bincode-serialised nodes.
/// v2: NodeKind stored as UTF-8 string in bincode-serialised nodes.
///
/// Handles two cases:
/// 1. DB was created fresh with v2 code — nodes deserialise correctly.
///    We just verify and the schema version key is already set.
/// 2. DB has genuine v1 data — deserialization fails. Requires an external
///    dump/restore (export from v1 binary, import via `cortex import`).
pub fn migrate_v1_to_v2(storage: &mut RedbStorage) -> anyhow::Result<()> {
    let db_path = storage.path().to_path_buf();
    let backup_path = db_path.with_extension("redb.v1.bak");

    // Backup before touching anything
    std::fs::copy(&db_path, &backup_path).map_err(|e| {
        anyhow::anyhow!("Failed to backup database to {:?}: {}", backup_path, e)
    })?;
    tracing::info!("Database backed up to {:?}", backup_path);

    // Attempt to read all nodes — success means DB is already v2 format
    let nodes = storage
        .list_nodes(NodeFilter::new().include_deleted())
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to read nodes — DB may contain genuine v1 (enum-encoded) NodeKind data. \
                To migrate: export with the v1 binary, then re-import using `cortex import`. \
                Error: {}",
                e
            )
        })?;

    tracing::info!(
        "Migration v1 → v2 complete: {} nodes verified",
        nodes.len()
    );

    Ok(())
}
