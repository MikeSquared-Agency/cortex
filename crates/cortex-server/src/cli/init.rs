use crate::config::{
    AutoLinkerTomlConfig, BriefingTomlConfig, CortexConfig, EmbeddingConfig, IngestConfig,
    ObservabilityConfig, RetentionConfig, ScoreDecayConfig, SchemaConfig, SecurityConfig,
    ServerConfig,
};
use anyhow::Result;
use cortex_core::briefing::BriefingRoleConfig;
use cortex_core::{ConfigRule, RuleCondition};

/// Default structural linking rules that replicate legacy hardcoded behaviour.
/// Used by `cortex init` so new instances start with configurable rules.
fn default_config_rules() -> Vec<ConfigRule> {
    vec![
        ConfigRule {
            name: "same-agent-linking".into(),
            from_kind: "*".into(),
            to_kind: "*".into(),
            relation: "related_to".into(),
            weight: 0.6,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::SameAgent,
        },
        ConfigRule {
            name: "temporal-proximity".into(),
            from_kind: "*".into(),
            to_kind: "*".into(),
            relation: "related_to".into(),
            weight: 0.5,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::TemporalProximity { window_minutes: 30 },
        },
        ConfigRule {
            name: "shared-tags".into(),
            from_kind: "*".into(),
            to_kind: "*".into(),
            relation: "related_to".into(),
            weight: 0.7,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::SharedTags { min_shared: 2 },
        },
        ConfigRule {
            name: "decision-leads-to-event".into(),
            from_kind: "decision".into(),
            to_kind: "event".into(),
            relation: "led_to".into(),
            weight: 0.8,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::TemporalProximity { window_minutes: 60 },
        },
        ConfigRule {
            name: "observation-instance-of-pattern".into(),
            from_kind: "observation".into(),
            to_kind: "pattern".into(),
            relation: "instance_of".into(),
            weight: 0.7,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::MinSimilarity { threshold: 0.75 },
        },
        ConfigRule {
            name: "fact-supersedes-fact".into(),
            from_kind: "fact".into(),
            to_kind: "fact".into(),
            relation: "supersedes".into(),
            weight: 0.9,
            weight_from_score: false,
            bidirectional: false,
            condition: RuleCondition::NewerThan,
        },
    ]
}

pub(crate) fn roles_for_template(template: &str) -> BriefingRoleConfig {
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
        "legal" => BriefingRoleConfig {
            identity: vec!["agent".into()],
            persistent: vec![
                "legal-precedent".into(),
                "regulation".into(),
                "client-profile".into(),
            ],
            trackable: vec!["case".into(), "filing".into(), "deadline".into()],
            temporal: vec![
                "hearing".into(),
                "deposition".into(),
                "correspondence".into(),
            ],
            reviewable: vec!["pattern".into(), "argument".into(), "citation".into()],
            superseding: vec![
                "case-status".into(),
                "client-statement".into(),
                "ruling".into(),
            ],
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

    let is_legal = template == Some("legal");
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
            decay_enabled: !is_legal,
            decay_rate_per_day: if is_legal { 0.0 } else { 0.01 },
            rules: default_config_rules(),
            ..AutoLinkerTomlConfig::default()
        },
        briefing: BriefingTomlConfig {
            precompute_agents: agents,
            roles,
            ..BriefingTomlConfig::default()
        },
        ingest: IngestConfig::default(),
        observability: ObservabilityConfig::default(),
        retention: if is_legal {
            RetentionConfig {
                default_ttl_days: 0,
                ..RetentionConfig::default()
            }
        } else {
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
        score_decay: if is_legal {
            ScoreDecayConfig {
                enabled: false,
                ..Default::default()
            }
        } else {
            Default::default()
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legal_template_roles() {
        let roles = roles_for_template("legal");
        assert!(roles.persistent.contains(&"legal-precedent".to_string()));
        assert!(roles.persistent.contains(&"regulation".to_string()));
        assert!(roles.persistent.contains(&"client-profile".to_string()));
        assert!(roles.trackable.contains(&"case".to_string()));
        assert!(roles.trackable.contains(&"filing".to_string()));
        assert!(roles.trackable.contains(&"deadline".to_string()));
        assert!(roles.temporal.contains(&"hearing".to_string()));
        assert!(roles.temporal.contains(&"deposition".to_string()));
        assert!(roles.reviewable.contains(&"citation".to_string()));
        assert!(roles.superseding.contains(&"ruling".to_string()));
    }

    #[test]
    fn test_legal_template_unknown_falls_back_to_default() {
        let legal = roles_for_template("legal");
        let default = roles_for_template("unknown");
        // Legal has specific roles, default does not
        assert!(legal.persistent.contains(&"regulation".to_string()));
        assert!(!default.persistent.contains(&"regulation".to_string()));
    }
}
