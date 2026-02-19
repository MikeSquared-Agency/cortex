pub mod backup;
pub mod briefing;
pub mod config_cmd;
pub mod doctor;
pub mod edge;
pub mod export;
pub mod import;
pub mod init;
pub mod migrate;
pub mod node;
pub mod search;
pub mod shell;
pub mod stats;
pub mod traverse;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cortex")]
#[command(version, about = "Embedded graph memory for AI agents")]
pub struct Cli {
    /// Path to cortex.toml
    #[arg(long, global = true, env = "CORTEX_CONFIG", default_value = "cortex.toml")]
    pub config: PathBuf,

    /// Path to data directory (overrides config file)
    #[arg(long, global = true, env = "CORTEX_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Cortex server address for client commands
    #[arg(long, global = true, env = "CORTEX_ADDR", default_value = "http://localhost:9090")]
    pub server: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the gRPC + HTTP server
    Serve,
    /// Interactive setup wizard
    Init,
    /// Interactive REPL
    Shell,
    /// Node operations
    #[command(subcommand)]
    Node(NodeCommands),
    /// Edge operations
    #[command(subcommand)]
    Edge(EdgeCommands),
    /// Search the graph
    Search(SearchArgs),
    /// Graph traversal from a node
    Traverse(TraverseArgs),
    /// Find shortest path between two nodes
    Path(PathArgs),
    /// Generate a context briefing
    Briefing(BriefingArgs),
    /// Import data into the graph
    Import(ImportArgs),
    /// Export graph data
    Export(ExportArgs),
    /// Back up the database
    Backup(BackupArgs),
    /// Restore from backup
    Restore(RestoreArgs),
    /// Run schema migrations
    Migrate,
    /// Graph statistics
    Stats,
    /// Diagnose issues
    Doctor,
    /// Configuration commands
    #[command(subcommand)]
    Config(ConfigCommands),
}

#[derive(Subcommand, Debug)]
pub enum NodeCommands {
    Create(NodeCreateArgs),
    Get(NodeGetArgs),
    List(NodeListArgs),
    Delete(NodeDeleteArgs),
}

#[derive(Subcommand, Debug)]
pub enum EdgeCommands {
    Create(EdgeCreateArgs),
    List(EdgeListArgs),
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    Validate,
    Show,
}

// --- Node args ---

#[derive(Args, Debug)]
pub struct NodeCreateArgs {
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long, default_value = "0.5")]
    pub importance: f32,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    /// Read body from stdin
    #[arg(long)]
    pub stdin: bool,
    /// Output format: table (default), json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct NodeGetArgs {
    pub id: String,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct NodeListArgs {
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long, default_value = "20")]
    pub limit: u32,
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct NodeDeleteArgs {
    pub id: String,
    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

// --- Edge args ---

#[derive(Args, Debug)]
pub struct EdgeCreateArgs {
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
    #[arg(long)]
    pub relation: String,
    #[arg(long, default_value = "1.0")]
    pub weight: f32,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct EdgeListArgs {
    #[arg(long)]
    pub node: String,
    /// "outgoing", "incoming", "both"
    #[arg(long, default_value = "both")]
    pub direction: String,
    #[arg(long, default_value = "table")]
    pub format: String,
}

// --- Search args ---

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value = "10")]
    pub limit: u32,
    /// Hybrid search (vector + graph)
    #[arg(long)]
    pub hybrid: bool,
    #[arg(long, default_value = "table")]
    pub format: String,
}

// --- Traverse / Path args ---

#[derive(Args, Debug)]
pub struct TraverseArgs {
    pub id: String,
    #[arg(long, default_value = "2")]
    pub depth: u32,
    /// "outgoing", "incoming", "both"
    #[arg(long, default_value = "both")]
    pub direction: String,
    #[arg(long)]
    pub relation: Option<String>,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PathArgs {
    pub from: String,
    pub to: String,
    #[arg(long, default_value = "5")]
    pub max_hops: u32,
    #[arg(long, default_value = "table")]
    pub format: String,
}

// --- Briefing args ---

#[derive(Args, Debug)]
pub struct BriefingArgs {
    pub agent_id: String,
    #[arg(long)]
    pub compact: bool,
    /// "text", "json", "markdown"
    #[arg(long, default_value = "text")]
    pub format: String,
    #[arg(long)]
    pub no_cache: bool,
}

// --- Import args ---

#[derive(Args, Debug)]
pub struct ImportArgs {
    pub file: PathBuf,
    /// "json", "jsonl", "csv", "markdown" — auto-detected if omitted
    #[arg(long)]
    pub format: Option<String>,
    #[arg(long, default_value = "import")]
    pub source: String,
    #[arg(long)]
    pub dry_run: bool,
}

// --- Export args ---

#[derive(Args, Debug)]
pub struct ExportArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// "json", "jsonl", "dot", "graphml"
    #[arg(long, default_value = "json")]
    pub format: String,
    #[arg(long)]
    pub kind: Option<String>,
}

// --- Backup / Restore args ---

#[derive(Args, Debug)]
pub struct BackupArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub encrypt: bool,
}

#[derive(Args, Debug)]
pub struct RestoreArgs {
    pub path: PathBuf,
    #[arg(long, short = 'y')]
    pub yes: bool,
}

// --- gRPC client helper ---

use cortex_proto::cortex_service_client::CortexServiceClient;
use tonic::transport::Channel;

pub async fn grpc_connect(server: &str) -> anyhow::Result<CortexServiceClient<Channel>> {
    CortexServiceClient::connect(server.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Cortex server at {}: {}\nIs `cortex serve` running?", server, e))
}

// --- Table printing helpers ---

pub fn print_node_table(nodes: &[cortex_proto::NodeResponse]) {
    if nodes.is_empty() {
        println!("(no results)");
        return;
    }
    println!("{:<36}  {:<12}  {:<6}  {}", "ID", "KIND", "IMP", "TITLE");
    println!("{}", "─".repeat(80));
    for n in nodes {
        let title = truncate(&n.title, 40);
        println!("{:<36}  {:<12}  {:<6.2}  {}", n.id, n.kind, n.importance, title);
    }
}

pub fn print_edge_table(edges: &[cortex_proto::EdgeResponse]) {
    if edges.is_empty() {
        println!("(no edges)");
        return;
    }
    println!("{:<36}  {:<36}  {:<20}  {:<5}", "FROM", "TO", "RELATION", "WEIGHT");
    println!("{}", "─".repeat(100));
    for e in edges {
        println!("{:<36}  {:<36}  {:<20}  {:<5.2}", e.from_id, e.to_id, e.relation, e.weight);
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}
