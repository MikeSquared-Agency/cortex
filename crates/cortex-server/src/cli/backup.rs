use crate::cli::{BackupArgs, RestoreArgs};
use crate::config::CortexConfig;
use anyhow::Result;
use std::path::Path;

pub async fn run(args: BackupArgs, config: CortexConfig) -> Result<()> {
    let db_path = config.db_path();

    if !db_path.exists() {
        anyhow::bail!("Database not found at {}", db_path.display());
    }

    println!(
        "Creating backup: {} → {}",
        db_path.display(),
        args.path.display()
    );

    // Ensure destination directory exists
    if let Some(parent) = args.path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Copy the redb file
    std::fs::copy(&db_path, &args.path)?;

    // Write SHA-256 checksum sidecar
    let checksum = sha256_file(&args.path)?;
    let checksum_path = args.path.with_extension("sha256");
    std::fs::write(
        &checksum_path,
        format!("{}  {}\n", checksum, args.path.display()),
    )?;

    if args.encrypt {
        eprintln!("Warning: --encrypt not yet implemented (CORTEX_ENCRYPTION_KEY not supported)");
    }

    println!("✅ Backup complete: {}", args.path.display());
    println!("   Checksum: {} ({})", checksum, checksum_path.display());

    Ok(())
}

pub async fn run_restore(args: RestoreArgs, config: CortexConfig) -> Result<()> {
    let backup_path = &args.path;

    if !backup_path.exists() {
        anyhow::bail!("Backup file not found: {}", backup_path.display());
    }

    // Verify checksum if sidecar exists
    let checksum_path = backup_path.with_extension("sha256");
    if checksum_path.exists() {
        let expected = std::fs::read_to_string(&checksum_path)?;
        let expected_hash = expected.split_whitespace().next().unwrap_or("").to_string();
        let actual_hash = sha256_file(backup_path)?;

        if expected_hash != actual_hash {
            anyhow::bail!(
                "Checksum mismatch!\n  Expected: {}\n  Actual:   {}",
                expected_hash,
                actual_hash
            );
        }
        println!("✅ Checksum verified: {}", actual_hash);
    } else {
        eprintln!("Warning: no .sha256 sidecar found, skipping checksum verification");
    }

    let db_path = config.db_path();

    if !args.yes {
        use inquire::Confirm;
        let confirmed = Confirm::new(&format!(
            "Restore {} to {}? This will overwrite existing data.",
            backup_path.display(),
            db_path.display()
        ))
        .with_default(false)
        .prompt()?;

        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Ensure destination directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::copy(backup_path, &db_path)?;
    println!(
        "✅ Restored {} to {}",
        backup_path.display(),
        db_path.display()
    );
    println!("   Run `cortex migrate` if you upgraded Cortex since this backup was made.");

    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}
