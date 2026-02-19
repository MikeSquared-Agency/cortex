use crate::cli::ConfigCommands;
use crate::config::CortexConfig;
use anyhow::Result;
use std::path::Path;

pub async fn run(cmd: ConfigCommands, config_path: &Path) -> Result<()> {
    match cmd {
        ConfigCommands::Validate => validate(config_path),
        ConfigCommands::Show => show(config_path),
    }
}

fn validate(config_path: &Path) -> Result<()> {
    match CortexConfig::load(config_path) {
        Ok(config) => {
            let errors = config.validate();
            if errors.is_empty() {
                println!("✅ {} is valid.", config_path.display());
            } else {
                println!("❌ Validation errors in {}:", config_path.display());
                for e in &errors {
                    println!("  - {}", e);
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("❌ Failed to parse {}: {}", config_path.display(), e);
            std::process::exit(1);
        }
    }
    Ok(())
}

fn show(config_path: &Path) -> Result<()> {
    let config = CortexConfig::load_or_default(config_path);
    match toml::to_string_pretty(&config) {
        Ok(s) => println!("{}", s),
        Err(e) => anyhow::bail!("Failed to serialize config: {}", e),
    }
    Ok(())
}
