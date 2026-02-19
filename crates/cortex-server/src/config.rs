use clap::Parser;
use cortex_core::{AutoLinkerConfig, SimilarityConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(name = "cortex-server")]
#[command(about = "Cortex graph memory engine server")]
pub struct Config {
    /// gRPC listen address
    #[arg(long, env = "CORTEX_GRPC_ADDR", default_value = "0.0.0.0:9090")]
    pub grpc_addr: SocketAddr,

    /// HTTP listen address
    #[arg(long, env = "CORTEX_HTTP_ADDR", default_value = "0.0.0.0:9091")]
    pub http_addr: SocketAddr,

    /// NATS URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    pub nats_url: String,

    /// Data directory
    #[arg(long, env = "CORTEX_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    /// Auto-linker interval in seconds
    #[arg(long, env = "CORTEX_AUTOLINK_INTERVAL", default_value = "60")]
    pub autolink_interval: u64,

    /// Similarity threshold for auto-linking
    #[arg(long, env = "CORTEX_AUTOLINK_THRESHOLD", default_value = "0.75")]
    pub autolink_threshold: f32,

    /// Enable NATS consumer
    #[arg(long, env = "CORTEX_NATS_ENABLED", default_value = "true")]
    pub nats_enabled: bool,

    /// Max message size for gRPC (bytes)
    #[arg(long, env = "CORTEX_MAX_MESSAGE_SIZE", default_value = "16777216")]
    pub max_message_size: usize,
}

impl Config {
    pub fn auto_linker_config(&self) -> AutoLinkerConfig {
        AutoLinkerConfig::new()
            .with_interval(Duration::from_secs(self.autolink_interval))
            .with_similarity(
                SimilarityConfig::new()
                    .with_auto_link_threshold(self.autolink_threshold)
            )
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("cortex.redb")
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.data_dir.exists() {
            std::fs::create_dir_all(&self.data_dir)?;
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:9090".parse().unwrap(),
            http_addr: "0.0.0.0:9091".parse().unwrap(),
            nats_url: "nats://localhost:4222".to_string(),
            data_dir: PathBuf::from("./data"),
            autolink_interval: 60,
            autolink_threshold: 0.75,
            nats_enabled: true,
            max_message_size: 16 * 1024 * 1024, // 16MB
        }
    }
}
