use crate::config::{
    AutoLinkerTomlConfig, BriefingTomlConfig, CortexConfig, EmbeddingConfig, IngestConfig,
    ObservabilityConfig, RetentionConfig, SchemaConfig, SecurityConfig, ServerConfig,
};
use anyhow::Result;
use cortex_core::briefing::BriefingRoleConfig;

fn roles_for_template(template: &str) -> BriefingRoleConfig {
    match template {
        "coding" => BriefingRoleConfig {
            identity: vec!["agent".into()],
            persistent: vec!["constraint".into(), "architecture-decision".into()],
            trackable: vec!["task".into(), "milestone".into()],
            temporal: vec!["commit".into(), "deployment".into(), "incident".into()],
            reviewable: vec!["pattern".into(), "anti-pattern".into(), "code-smell".into()],
            superseding: vec!["dependency".into(), "api-version".into()],
        },
        "research" => BriefingRoleConfig {
            identity: vec!["agent".into()],
            persistent: vec!["hypothesis".into(), "methodology".into()],
            trackable: vec!["research-question".into(), "objective".into()],
            temporal: vec!["experiment".into(), "observation".into()],
            reviewable: vec!["finding".into(), "pattern".into()],
            superseding: vec!["claim".into(), "measurement".into(), "citation".into()],
        },
        "browser" => BriefingRoleConfig {
            identity: vec!["agent".into()],
            persistent: vec!["preference".into(), "bookmark".into()],
            trackable: vec!["task".into(), "search-query".into()],
            temporal: vec!["page-visit".into(), "extraction".into()],
            reviewable: vec!["pattern".into(), "site-profile".into()],
            superseding: vec!["fact".into(), "screenshot".into()],
        },
        _ => BriefingRoleConfig::default(),
    }
}

pub async fn run(template: Option<&str>) -> Result<()> {
    use inquire::{Confirm, Select, Text};

    println!("\nWelcome to Cortex — graph memory for AI agents.\n");

    if let Some(t) = template {
        println!("Using template: {}\n", t);
    }

    let data_dir = Text::new("Where should Cortex store data?")
        .with_default("./data")
        .prompt()?;

    let embedding_model = Select::new(
        "Which embedding model?",
        vec![
            "BAAI/bge-small-en-v1.5 (384d, fast, English)",
            "BAAI/bge-base-en-v1.5 (768d, balanced)",
            "BAAI/bge-large-en-v1.5 (1024d, accurate)",
        ],
    )
    .prompt()?;

    let autolinker = Confirm::new("Enable auto-linker?")
        .with_default(true)
        .prompt()?;

    let autolinker_interval = if autolinker {
        Text::new("Auto-linker interval (seconds)?")
            .with_default("60")
            .prompt()?
            .parse::<u64>()
            .unwrap_or(60)
    } else {
        60
    };

    let ingest_choice = Select::new(
        "Enable event ingest?",
        vec!["None", "File watcher", "Webhook endpoint", "NATS"],
    )
    .prompt()?;

    let agents_str = Text::new("Pre-configure agent briefings? Enter agent IDs (comma-separated):")
        .with_default("default")
        .prompt()?;
    let agents: Vec<String> = agents_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let http_debug = Confirm::new("Enable HTTP debug server?")
        .with_default(true)
        .prompt()?;

    // Build config
    let model_name = if embedding_model.contains("bge-base") {
        "BAAI/bge-base-en-v1.5"
    } else if embedding_model.contains("bge-large") {
        "BAAI/bge-large-en-v1.5"
    } else {
        "BAAI/bge-small-en-v1.5"
    };

    let grpc_addr = "0.0.0.0:9090".to_string();
    let http_addr = if http_debug {
        "0.0.0.0:9091".to_string()
    } else {
        "127.0.0.1:9091".to_string()
    };

    let nats_enabled = ingest_choice == "NATS";

    let roles = roles_for_template(template.unwrap_or("default"));

    let config = CortexConfig {
        server: ServerConfig {
            grpc_addr,
            http_addr,
            data_dir: std::path::PathBuf::from(&data_dir),
            nats_url: "nats://localhost:4222".into(),
            nats_enabled,
            max_message_size: 16 * 1024 * 1024,
        },
        schema: SchemaConfig::default(),
        embedding: EmbeddingConfig {
            model: model_name.into(),
        },
        auto_linker: AutoLinkerTomlConfig {
            enabled: autolinker,
            interval_seconds: autolinker_interval,
            ..AutoLinkerTomlConfig::default()
        },
        briefing: BriefingTomlConfig {
            precompute_agents: agents,
            roles,
            ..BriefingTomlConfig::default()
        },
        ingest: IngestConfig::default(),
        observability: ObservabilityConfig::default(),
        retention: {
            let mut r = RetentionConfig::default();
            r.by_kind.insert(
                "observation".to_string(),
                cortex_core::KindRetention {
                    ttl_days: 90,
                    min_score: None,
                },
            );
            r
        },
        security: SecurityConfig::default(),
        webhooks: vec![],
        plugins: vec![],
        prompt_rollback: Default::default(),
        score_decay: Default::default(),
        write_gate: Default::default(),
        schemas: Default::default(),
        trust: None,
    };

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write("cortex.toml", &toml_str)?;
    println!("\n✅ Generated cortex.toml");

    std::fs::create_dir_all(&data_dir)?;
    println!("✅ Created data directory: {}", data_dir);

    if template.is_some() {
        println!("✅ Applied briefing role template");
    }
    println!("✅ Ready\n");
    println!("Run `cortex serve` to start, or `cortex shell` for interactive mode.");

    Ok(())
}
