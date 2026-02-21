/// One-shot database repair utility.
///
/// Usage: repair_db <path-to-cortex.redb>
///
/// Uses redb's built-in repair callback to recover from unclean shutdowns.
/// A backup is written alongside the original before any repair is attempted.
fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/home/mike/.cortex/data/cortex.redb".to_string());
    let path = std::path::PathBuf::from(&path);

    if !path.exists() {
        anyhow::bail!("Database file not found: {}", path.display());
    }

    // Make a backup before touching anything
    let backup = path.with_extension("redb.bak");
    println!("Backing up {} → {}", path.display(), backup.display());
    std::fs::copy(&path, &backup)?;
    println!("Backup written ({} bytes)", std::fs::metadata(&backup)?.len());

    // Attempt to open with repair callback enabled
    println!("Opening database with repair enabled...");
    let db = redb::Database::builder()
        .set_repair_callback(|session| {
            println!("  Repair progress: {:.0}%", session.progress() * 100.0);
        })
        .open(&path)?;

    println!("Database opened. Running integrity check...");
    let mut db = db;
    match db.check_integrity() {
        Ok(true) => println!("Integrity check passed — database is healthy."),
        Ok(false) => println!("Integrity check found issues but repair succeeded."),
        Err(e) => {
            println!("Integrity check failed after repair attempt: {e}");
            println!("The backup is preserved at: {}", backup.display());
            anyhow::bail!("Could not repair: {e}");
        }
    }

    println!("Done. Start cortex normally.");
    Ok(())
}
