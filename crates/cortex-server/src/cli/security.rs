use super::SecurityCommands;
use anyhow::Result;

pub async fn run(cmd: SecurityCommands) -> Result<()> {
    match cmd {
        SecurityCommands::GenerateKey => {
            let key = cortex_core::storage::encrypted::generate_key();
            println!();
            println!("Generated a new 256-bit AES encryption key.");
            println!("Add to your environment:");
            println!();
            println!("  export CORTEX_ENCRYPTION_KEY=\"{}\"", key);
            println!();
            println!(
                "Keep this key safe — data encrypted with it cannot be recovered without it."
            );
            println!("Store it in a password manager or secrets vault.");
        }
    }
    Ok(())
}
