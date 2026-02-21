pub mod agent;
pub mod audit;
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
pub mod prompt;
pub mod search;
pub mod security;
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
    #[arg(
        long,
        global = true,
        env = "CORTEX_CONFIG",
        default_value = "cortex.toml"
    )]
    pub config: PathBuf,

    /// Path to data directory (overrides config file)
    #[arg(long, global = true, env = "CORTEX_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Cortex server address for client commands
    #[arg(
        long,
        global = true,
        env = "CORTEX_ADDR",
        default_value = "http://localhost:9090"
    )]
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
    /// Query the audit log
    Audit(AuditArgs),
    /// Security utilities (key generation, etc.)
    #[command(subcommand)]
    Security(SecurityCommands),
    /// Start an MCP server (stdio transport for AI agent integration)
    Mcp(McpArgs),
    /// Agent ↔ prompt binding management
    #[command(subcommand)]
    Agent(AgentCommands),
    /// Prompt versioning, branching, and migration (PromptForge integration)
    #[command(subcommand)]
    Prompt(PromptCommands),
}

// --- MCP args ---

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Path to cortex data directory. Defaults to ~/.cortex/default or the global --data-dir.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Connect to a running Cortex server via gRPC instead of opening the database directly.
    #[arg(long)]
    pub server: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum NodeCommands {
    Create(NodeCreateArgs),
    Get(NodeGetArgs),
    List(NodeListArgs),
    Delete(NodeDeleteArgs),
    /// Show access-tracking stats for a node (access count, last accessed, decay info)
    Stats(NodeStatsArgs),
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

#[derive(Subcommand, Debug)]
pub enum SecurityCommands {
    /// Generate a new 256-bit AES encryption key
    GenerateKey,
}

// --- Agent args ---

#[derive(Subcommand, Debug)]
pub enum AgentCommands {
    /// List all agent nodes
    List(AgentListArgs),
    /// Show prompts bound to an agent
    Show(AgentShowArgs),
    /// Bind (or update weight of) a prompt to an agent
    Bind(AgentBindArgs),
    /// Unbind a prompt from an agent
    Unbind(AgentUnbindArgs),
    /// Show the fully resolved effective prompt for an agent
    Resolve(AgentResolveArgs),
    /// Select the best prompt variant for the current context (epsilon-greedy)
    Select(AgentSelectArgs),
    /// Show variant swap and performance history
    History(AgentHistoryArgs),
    /// Record a performance observation and update edge weights
    Observe(AgentObserveArgs),
}

#[derive(Args, Debug)]
pub struct AgentListArgs {
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentShowArgs {
    /// Agent name (title of the agent node)
    pub name: String,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentBindArgs {
    /// Agent name
    pub name: String,
    /// Prompt slug (title of the prompt node)
    pub slug: String,
    /// Edge weight [0.0–1.0]; higher = more important
    #[arg(long, default_value = "1.0")]
    pub weight: f32,
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentUnbindArgs {
    /// Agent name
    pub name: String,
    /// Prompt slug
    pub slug: String,
}

#[derive(Args, Debug)]
pub struct AgentResolveArgs {
    /// Agent name
    pub name: String,
    /// Output format: text (default) | json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentSelectArgs {
    /// Agent name
    pub name: String,
    /// User sentiment: 0.0 (frustrated) – 1.0 (pleased)
    #[arg(long, default_value = "0.5")]
    pub sentiment: f32,
    /// Task type: coding | planning | casual | crisis | reflection
    #[arg(long, default_value = "casual")]
    pub task_type: String,
    /// Correction rate (0.0–1.0)
    #[arg(long, default_value = "0.0")]
    pub correction_rate: f32,
    /// Topic shift from conversation start (0.0–1.0)
    #[arg(long, default_value = "0.0")]
    pub topic_shift: f32,
    /// User energy proxy (0.0–1.0)
    #[arg(long, default_value = "0.5")]
    pub energy: f32,
    /// Exploration rate for epsilon-greedy (0.0 = always exploit)
    #[arg(long, default_value = "0.2")]
    pub epsilon: f32,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentHistoryArgs {
    /// Agent name
    pub name: String,
    /// Maximum number of history entries to show
    #[arg(long, default_value = "20")]
    pub limit: usize,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct AgentObserveArgs {
    /// Agent name
    pub name: String,
    /// UUID of the prompt variant node that was active
    #[arg(long)]
    pub variant_id: String,
    /// Slug of the prompt variant (for display)
    #[arg(long)]
    pub variant_slug: String,
    /// Observed sentiment score: 0.0–1.0
    #[arg(long, default_value = "0.5")]
    pub sentiment_score: f32,
    /// Number of corrections the user made
    #[arg(long, default_value = "0")]
    pub correction_count: u32,
    /// Task outcome: success | partial | failure | unknown
    #[arg(long, default_value = "unknown")]
    pub task_outcome: String,
    /// Token cost of the interaction
    #[arg(long)]
    pub token_cost: Option<u32>,
}

// --- Prompt args ---

#[derive(Subcommand, Debug)]
pub enum PromptCommands {
    /// List all prompts (HEAD of each slug+branch)
    List(PromptListArgs),
    /// Show a prompt (resolved with inheritance by default)
    Get(PromptGetArgs),
    /// Import prompts from a migration JSON file
    Migrate(PromptMigrateArgs),
    /// Show aggregate performance metrics for a prompt variant
    Performance(PromptPerformanceArgs),
    /// Record a deployment and snapshot baseline metrics for rollback monitoring
    Deploy(PromptDeployArgs),
    /// Show rollback status, cooldown, and quarantine state for a prompt
    RollbackStatus(PromptRollbackStatusArgs),
    /// Remove quarantine from a prompt version (allows re-evaluation)
    Unquarantine(PromptUnquarantineArgs),
}

#[derive(Args, Debug)]
pub struct PromptListArgs {
    /// Filter by branch
    #[arg(long)]
    pub branch: Option<String>,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PromptGetArgs {
    /// Prompt slug
    pub slug: String,
    /// Branch (default: main)
    #[arg(long)]
    pub branch: Option<String>,
    /// Specific version number (omit for HEAD)
    #[arg(long)]
    pub version: Option<u32>,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PromptMigrateArgs {
    /// Path to migration JSON file
    pub file: std::path::PathBuf,
    /// Preview without writing to the database
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct PromptPerformanceArgs {
    /// Prompt slug
    pub slug: String,
    /// Maximum observations to include
    #[arg(long, default_value = "50")]
    pub limit: usize,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PromptDeployArgs {
    /// Prompt slug to deploy
    pub slug: String,
    /// Branch (default: main)
    #[arg(long, default_value = "main")]
    pub branch: String,
    /// Agent name responsible for this deployment
    #[arg(long)]
    pub agent_name: String,
    /// Number of recent observations to use for baseline (default: 20)
    #[arg(long, default_value = "20")]
    pub baseline_sample_size: usize,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PromptRollbackStatusArgs {
    /// Prompt slug
    pub slug: String,
    /// Branch (default: main)
    #[arg(long, default_value = "main")]
    pub branch: String,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
}

#[derive(Args, Debug)]
pub struct PromptUnquarantineArgs {
    /// Prompt slug
    pub slug: String,
    /// Branch (default: main)
    #[arg(long, default_value = "main")]
    pub branch: String,
}

// --- Audit args ---

#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Only show entries since this duration (e.g. "24h", "7d", "1h30m")
    #[arg(long)]
    pub since: Option<String>,
    /// Filter by node/edge ID
    #[arg(long)]
    pub node: Option<String>,
    /// Filter by actor name (e.g. "kai", "auto-linker")
    #[arg(long)]
    pub actor: Option<String>,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
    /// Maximum number of entries to return
    #[arg(long, default_value = "100")]
    pub limit: usize,
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

#[derive(Args, Debug)]
pub struct NodeStatsArgs {
    pub id: String,
    #[arg(long, default_value = "table")]
    pub format: String,
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
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to connect to Cortex server at {}: {}\nIs `cortex serve` running?",
                server,
                e
            )
        })
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
        println!(
            "{:<36}  {:<12}  {:<6.2}  {}",
            n.id, n.kind, n.importance, title
        );
    }
}

pub fn print_edge_table(edges: &[cortex_proto::EdgeResponse]) {
    if edges.is_empty() {
        println!("(no edges)");
        return;
    }
    println!(
        "{:<36}  {:<36}  {:<20}  {:<5}",
        "FROM", "TO", "RELATION", "WEIGHT"
    );
    println!("{}", "─".repeat(100));
    for e in edges {
        println!(
            "{:<36}  {:<36}  {:<20}  {:<5.2}",
            e.from_id, e.to_id, e.relation, e.weight
        );
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}
