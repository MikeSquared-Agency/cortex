#![allow(dead_code)]
mod briefing;
mod cli;
mod config;
mod grpc;
mod http;
mod ingest;
mod mcp;
mod migration;
mod serve;

#[cfg(feature = "warren")]
mod nats;

use clap::Parser;
use cli::{Cli, Commands};
use config::CortexConfig;
use tracing::error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    // Load config for commands that need it (serve, local ops)
    let mut config = CortexConfig::load_or_default(&cli.config);

    // Override data_dir if specified on command line
    if let Some(data_dir) = &cli.data_dir {
        config.server.data_dir = data_dir.clone();
    }

    match cli.command {
        Commands::Serve => {
            config.ensure_data_dir()?;
            let errors = config.validate();
            if !errors.is_empty() {
                for e in &errors {
                    error!("Config error: {}", e);
                }
                anyhow::bail!("Invalid cortex.toml configuration");
            }
            serve::run(config).await?;
        }

        Commands::Init => {
            cli::init::run().await?;
        }

        Commands::Shell => {
            cli::shell::run(config, &cli.server, &cli.config).await?;
        }

        Commands::Node(cmd) => {
            cli::node::run(cmd, &cli.server).await?;
        }

        Commands::Edge(cmd) => {
            cli::edge::run(cmd, &cli.server).await?;
        }

        Commands::Search(a) => {
            cli::search::run(a, &cli.server).await?;
        }

        Commands::Traverse(a) => {
            cli::traverse::run(a, &cli.server).await?;
        }

        Commands::Path(a) => {
            cli::traverse::run_path(a, &cli.server).await?;
        }

        Commands::Briefing(a) => {
            cli::briefing::run(a, &cli.server).await?;
        }

        Commands::Import(a) => {
            cli::import::run(a, config).await?;
        }

        Commands::Export(a) => {
            cli::export::run(a, &cli.server).await?;
        }

        Commands::Backup(a) => {
            cli::backup::run(a, config).await?;
        }

        Commands::Restore(a) => {
            cli::backup::run_restore(a, config).await?;
        }

        Commands::Migrate => {
            cli::migrate::run(config).await?;
        }

        Commands::Stats => {
            cli::stats::run(&cli.server).await?;
        }

        Commands::Doctor => {
            cli::doctor::run(config, &cli.server).await?;
        }

        Commands::Config(cmd) => {
            cli::config_cmd::run(cmd, &cli.config).await?;
        }

        Commands::Audit(args) => {
            cli::audit::run(args, config).await?;
        }

        Commands::Security(cmd) => {
            cli::security::run(cmd).await?;
        }

        Commands::Mcp(args) => {
            // MCP uses library mode — redirect tracing to stderr so stdout stays clean
            let data_dir = args
                .data_dir
                .or_else(|| Some(config.server.data_dir.clone()));
            let server = args.server;
            mcp::run(mcp::McpArgs { data_dir, server }).await?;
        }

        Commands::Agent(cmd) => {
            cli::agent::run(cmd, &cli.server).await?;
        }
    }

    Ok(())
}
