use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use crate::config::CortexConfig;

pub async fn run(config: CortexConfig, server: &str, config_path: &std::path::PathBuf) -> Result<()> {
    let mut rl = DefaultEditor::new()?;

    println!();
    println!("Cortex shell — connected to {}", server);
    println!("Type 'help' for available commands, 'exit' to quit.");
    println!();

    loop {
        match rl.readline("cortex> ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&line);

                if line == "exit" || line == "quit" {
                    println!("Goodbye.");
                    break;
                }

                if line == "help" {
                    print_help();
                    continue;
                }

                // Build argv: ["cortex", "--config", <path>, "--server", <server>, ...words]
                let mut argv = vec![
                    "cortex".to_string(),
                    "--config".to_string(),
                    config_path.display().to_string(),
                    "--server".to_string(),
                    server.to_string(),
                ];

                // Split the line respecting quoted strings
                let words = shell_split(&line);
                argv.extend(words);

                // Re-parse and dispatch
                use clap::Parser;
                match super::Cli::try_parse_from(&argv) {
                    Ok(cli) => {
                        if matches!(cli.command, super::Commands::Shell) {
                            println!("Already in shell mode.");
                            continue;
                        }
                        if let Err(e) = dispatch(cli, config.clone(), server, config_path).await {
                            eprintln!("Error: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(e) => {
                eprintln!("readline error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn dispatch(
    cli: super::Cli,
    config: CortexConfig,
    server: &str,
    config_path: &std::path::PathBuf,
) -> Result<()> {
    use super::Commands;
    match cli.command {
        Commands::Serve      => println!("Use 'exit' first, then run `cortex serve`."),
        Commands::Init       => super::init::run().await?,
        Commands::Shell      => println!("Already in shell mode."),
        Commands::Node(cmd)  => super::node::run(cmd, server).await?,
        Commands::Edge(cmd)  => super::edge::run(cmd, server).await?,
        Commands::Search(a)  => super::search::run(a, server).await?,
        Commands::Traverse(a)=> super::traverse::run(a, server).await?,
        Commands::Path(a)    => super::traverse::run_path(a, server).await?,
        Commands::Briefing(a)=> super::briefing::run(a, server).await?,
        Commands::Import(a)  => super::import::run(a, config).await?,
        Commands::Export(a)  => super::export::run(a, server).await?,
        Commands::Backup(a)  => super::backup::run(a, config).await?,
        Commands::Restore(a) => super::backup::run_restore(a, config).await?,
        Commands::Migrate    => super::migrate::run(config).await?,
        Commands::Stats      => super::stats::run(server).await?,
        Commands::Doctor     => super::doctor::run(config, server).await?,
        Commands::Config(cmd)=> super::config_cmd::run(cmd, config_path).await?,
        Commands::Audit(a)   => super::audit::run(a, config).await?,
        Commands::Security(c)=> super::security::run(c).await?,
    }
    Ok(())
}

fn print_help() {
    println!("Available commands (same as CLI, without 'cortex' prefix):");
    println!("  node create --kind <kind> --title <title> [--body <body>]");
    println!("  node get <id>");
    println!("  node list [--kind <kind>] [--limit N]");
    println!("  node delete <id>");
    println!("  edge create --from <id> --to <id> --relation <rel>");
    println!("  edge list --node <id>");
    println!("  search <query> [--hybrid] [--limit N]");
    println!("  traverse <id> [--depth N]");
    println!("  path <from> <to>");
    println!("  briefing <agent_id> [--compact]");
    println!("  import <file>");
    println!("  export [--format json|jsonl|dot|graphml]");
    println!("  backup <path>");
    println!("  restore <path>");
    println!("  migrate");
    println!("  stats");
    println!("  doctor");
    println!("  config validate|show");
    println!("  exit / quit");
}

/// Simple shell-like word splitting (handles quoted strings).
fn shell_split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in line.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
        } else if ch == ' ' || ch == '\t' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}
