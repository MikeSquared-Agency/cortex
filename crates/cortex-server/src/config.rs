use std::collections::HashMap;

use cortex_core::{AutoLinkerConfig, ConfigRule, NodeKind, Relation, SimilarityConfig};

// Re-export from cortex-core so cortex-server code can use them from config
#[allow(unused_imports)]
pub use cortex_core::gate::schema::{FieldSchema, FieldType, KindSchema};
pub use cortex_core::policies::RetentionConfig;
#[allow(unused_imports)]
pub use cortex_core::policies::RetentionMaxNodes;
pub use cortex_core::prompt::RollbackConfig;
pub use cortex_core::ScoreDecayConfig;
pub use cortex_core::WriteGateConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Top-level config, parsed from cortex.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CortexConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub schema: SchemaConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub auto_linker: AutoLinkerTomlConfig,
    #[serde(default)]
    pub briefing: BriefingTomlConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
    #[serde(default)]
    pub prompt_rollback: RollbackConfig,
    #[serde(default)]
    pub score_decay: ScoreDecayConfig,
    #[serde(default)]
    pub write_gate: WriteGateConfig,
    #[serde(default)]
    pub schemas: HashMap<String, KindSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub grpc_addr: String,
    pub http_addr: String,
    pub data_dir: PathBuf,
    pub nats_url: String,
    pub nats_enabled: bool,
    pub max_message_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:9090".into(),
            http_addr: "0.0.0.0:9091".into(),
            data_dir: PathBuf::from("./data"),
            nats_url: "nats://localhost:4222".into(),
            nats_enabled: true,
            max_message_size: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SchemaConfig {
    /// Registered node kinds. Defaults to the 8 built-in kinds.
    pub node_kinds: Vec<String>,
    /// Registered relation types. Defaults to the 8 built-in relations.
    pub relations: Vec<String>,
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            node_kinds: vec![
                "agent".into(),
                "decision".into(),
                "fact".into(),
                "event".into(),
                "goal".into(),
                "preference".into(),
                "pattern".into(),
                "observation".into(),
            ],
            relations: vec![
                "informed_by".into(),
                "led_to".into(),
                "applies_to".into(),
                "contradicts".into(),
                "supersedes".into(),
                "depends_on".into(),
                "related_to".into(),
                "instance_of".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "BAAI/bge-small-en-v1.5".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoLinkerTomlConfig {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub similarity_threshold: f32,
    pub dedup_threshold: f32,
    pub decay_rate_per_day: f32,
    pub max_edges_per_node: usize,
    /// Whether to run legacy hardcoded structural rules.
    /// None = auto (true when no rules defined, false when rules exist).
    pub legacy_rules_enabled: Option<bool>,
    /// User-defined structural linking rules.
    #[serde(default)]
    pub rules: Vec<ConfigRule>,
}

impl Default for AutoLinkerTomlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 60,
            similarity_threshold: 0.75,
            dedup_threshold: 0.92,
            decay_rate_per_day: 0.01,
            max_edges_per_node: 50,
            legacy_rules_enabled: None,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BriefingTomlConfig {
    pub cache_ttl_seconds: u64,
    pub max_total_items: usize,
    pub max_chars: usize,
    pub precompute_agents: Vec<String>,
    pub sections: Vec<BriefingSectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefingSectionConfig {
    pub name: String,
    /// "filter" | "traversal" | "hybrid_search" | "contradictions"
    pub mode: String,
    pub query: Option<String>,
    pub max_items: Option<usize>,
    pub sort: Option<String>,
    pub vector_weight: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IngestConfig {
    pub nats: Option<NatsIngestConfig>,
    pub webhook: Option<WebhookIngestConfig>,
    pub file: Option<FileIngestConfig>,
    pub stdin: Option<StdinIngestConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NatsIngestConfig {
    pub url: String,
    pub subjects: Vec<String>,
}

impl Default for NatsIngestConfig {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".into(),
            subjects: vec!["cortex.>".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebhookIngestConfig {
    pub enabled: bool,
    pub port: u16,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FileIngestConfig {
    pub watch_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StdinIngestConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObservabilityConfig {
    pub prometheus: bool,
    pub prometheus_port: u16,
    pub opentelemetry: bool,
    pub otlp_endpoint: Option<String>,
    /// Enable the /metrics endpoint (Prometheus text format). Default: true.
    pub metrics_enabled: bool,
    /// Require bearer token auth on /metrics. Default: false (Prometheus scrapes unauthenticated).
    pub metrics_require_auth: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            prometheus: false,
            prometheus_port: 0,
            opentelemetry: false,
            otlp_endpoint: None,
            metrics_enabled: true,
            metrics_require_auth: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SecurityConfig {
    pub encryption: bool,
    // Key comes from CORTEX_ENCRYPTION_KEY env var, never stored in config
    pub auth_enabled: bool,
    /// Fallback inline token. Prefer CORTEX_AUTH_TOKEN env var.
    pub auth_token: Option<String>,
}

impl SecurityConfig {
    /// Resolve the auth token: env var takes priority over inline config value.
    pub fn resolved_token(&self) -> Option<String> {
        std::env::var("CORTEX_AUTH_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| self.auth_token.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub events: Vec<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub path: PathBuf,
    /// "ingest" | "linker_rule" | "briefing_section" | "export_format" | "classifier"
    pub r#type: String,
}

impl CortexConfig {
    /// Load from a cortex.toml file.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: CortexConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from cortex.toml if it exists, otherwise use defaults.
    pub fn load_or_default(path: &std::path::Path) -> Self {
        if path.exists() {
            Self::load(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Validate the config. Returns a list of errors if invalid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for kind in &self.schema.node_kinds {
            if let Err(e) = NodeKind::new(kind) {
                errors.push(format!("schema.node_kinds: {}", e));
            }
        }
        for rel in &self.schema.relations {
            if let Err(e) = Relation::new(rel) {
                errors.push(format!("schema.relations: {}", e));
            }
        }
        // Validate auto-linker rules
        for rule in &self.auto_linker.rules {
            if let Err(e) = rule.validate() {
                errors.push(format!("auto_linker.rules: {}", e));
            }
        }
        errors
    }

    /// Ensure the data directory exists.
    pub fn ensure_data_dir(&self) -> anyhow::Result<()> {
        if !self.server.data_dir.exists() {
            std::fs::create_dir_all(&self.server.data_dir)?;
        }
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        self.server.data_dir.join("cortex.redb")
    }

    pub fn grpc_addr(&self) -> std::net::SocketAddr {
        self.server
            .grpc_addr
            .parse()
            .unwrap_or_else(|_| "0.0.0.0:9090".parse().unwrap())
    }

    pub fn http_addr(&self) -> std::net::SocketAddr {
        self.server
            .http_addr
            .parse()
            .unwrap_or_else(|_| "0.0.0.0:9091".parse().unwrap())
    }

    pub fn auto_linker_config(&self) -> AutoLinkerConfig {
        let mut config = AutoLinkerConfig::new()
            .with_interval(Duration::from_secs(self.auto_linker.interval_seconds))
            .with_similarity(
                SimilarityConfig::new()
                    .with_auto_link_threshold(self.auto_linker.similarity_threshold)
                    .with_dedup_threshold(self.auto_linker.dedup_threshold),
            )
            .with_decay(
                cortex_core::DecayConfig::new()
                    .with_daily_decay_rate(self.auto_linker.decay_rate_per_day),
            )
            .with_embedding_model(self.embedding.model.clone())
            .with_rules(self.auto_linker.rules.clone());

        if let Some(enabled) = self.auto_linker.legacy_rules_enabled {
            config = config.with_legacy_rules_enabled(enabled);
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_config_deserialization() {
        let toml_str = r#"
[schemas.decision]
required_fields = ["rationale"]

[schemas.decision.fields.priority]
type = "number"
min = 1.0
max = 5.0
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert!(config.schemas.contains_key("decision"));
        let schema = &config.schemas["decision"];
        assert_eq!(schema.required_fields, vec!["rationale".to_string()]);
        assert!(schema.fields.contains_key("priority"));
        let priority = &schema.fields["priority"];
        assert_eq!(priority.field_type, Some(FieldType::Number));
        assert_eq!(priority.min, Some(1.0));
        assert_eq!(priority.max, Some(5.0));
    }

    #[test]
    fn test_empty_schemas_default() {
        let config = CortexConfig::default();
        assert!(config.schemas.is_empty());
    }

    #[test]
    fn test_auto_linker_rules_deserialization() {
        let toml_str = r#"
[auto_linker]
enabled = true
legacy_rules_enabled = false

[[auto_linker.rules]]
name = "experiment-targets-function"
from_kind = "experiment"
to_kind = "function"
relation = "targets"
weight = 0.8
condition = { type = "shared_tags", min_shared = 1 }

[[auto_linker.rules]]
name = "fact-supersedes"
from_kind = "fact"
to_kind = "fact"
relation = "supersedes"
weight = 0.9
condition = { type = "newer_than" }
bidirectional = false

[[auto_linker.rules]]
name = "similar-functions"
from_kind = "function"
to_kind = "function"
relation = "similar_to"
weight_from_score = true
condition = { type = "min_similarity", threshold = 0.85 }
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.auto_linker.rules.len(), 3);
        assert_eq!(config.auto_linker.legacy_rules_enabled, Some(false));

        assert_eq!(config.auto_linker.rules[0].name, "experiment-targets-function");
        assert_eq!(config.auto_linker.rules[1].relation, "supersedes");
        assert!(config.auto_linker.rules[2].weight_from_score);

        // Verify it converts to AutoLinkerConfig correctly
        let linker_config = config.auto_linker_config();
        assert_eq!(linker_config.rules.len(), 3);
        assert!(!linker_config.use_legacy_rules());
    }

    #[test]
    fn test_auto_linker_no_rules_preserves_legacy() {
        let toml_str = r#"
[auto_linker]
enabled = true
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert!(config.auto_linker.rules.is_empty());
        assert_eq!(config.auto_linker.legacy_rules_enabled, None);

        let linker_config = config.auto_linker_config();
        assert!(linker_config.use_legacy_rules());
    }

    #[test]
    fn test_auto_linker_rules_validation() {
        let config = CortexConfig::default();
        let errors = config.validate();
        assert!(errors.is_empty());
    }
}
