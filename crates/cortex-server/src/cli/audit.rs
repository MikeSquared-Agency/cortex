use super::AuditArgs;
use crate::config::CortexConfig;
use anyhow::Result;

pub async fn run(_args: AuditArgs, _config: CortexConfig) -> Result<()> {
    println!("Audit log not yet implemented. Coming in Phase 7E.");
    Ok(())
}
