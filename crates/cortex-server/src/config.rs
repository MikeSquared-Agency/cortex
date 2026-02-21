use cortex_core::{AutoLinkerConfig, NodeKind, Relation, SimilarityConfig};

// Re-export from cortex-core so cortex-server code can use them from config
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ObservabilityConfig {
    pub prometheus: bool,
    pub prometheus_port: u16,
    pub opentelemetry: bool,
    pub otlp_endpoint: Option<String>,
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
        AutoLinkerConfig::new()
            .with_interval(Duration::from_secs(self.auto_linker.interval_seconds))
            .with_similarity(
                SimilarityConfig::new()
                    .with_auto_link_threshold(self.auto_linker.similarity_threshold),
            )
    }
}
