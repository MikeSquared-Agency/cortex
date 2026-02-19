# Phase 7B: CLI & Library Mode

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires `CortexConfig`, string newtypes, `IngestAdapter` trait.  
**Weeks:** 3–4  

---

## Overview

Turn the `cortex` binary into a comprehensive CLI toolkit and expose a `Cortex::open()` library API so applications can embed Cortex without running a server. This is the SQLite model: most users won't run `cortex serve` — they'll embed the library or use CLI commands directly.

---

## Current State

```
crates/cortex-server/src/main.rs   — binary entry point, currently only starts the gRPC+HTTP server
crates/cortex-core/src/lib.rs      — core library, no top-level convenience API
```

The binary has no subcommand structure — it is purely a server.

---

## Target State

```
crates/cortex-server/src/
  main.rs         — entry point, dispatches subcommands
  cli/
    mod.rs        — Cli struct + top-level clap config
    init.rs       — cortex init wizard
    shell.rs      — cortex shell REPL
    node.rs       — cortex node {create,get,list,delete}
    edge.rs       — cortex edge {create,list}
    search.rs     — cortex search
    traverse.rs   — cortex traverse / cortex path
    briefing.rs   — cortex briefing
    import.rs     — cortex import
    export.rs     — cortex export
    backup.rs     — cortex backup / cortex restore
    doctor.rs     — cortex doctor
    stats.rs      — cortex stats
    migrate.rs    — cortex migrate
    config_cmd.rs — cortex config {validate,show}
```

```
crates/cortex-core/src/
  api.rs          — Cortex struct (library mode public API)
```

---

## Full Command Reference

```
cortex serve                         Start the gRPC + HTTP server
cortex init                          Interactive setup wizard (generates cortex.toml)
cortex shell                         Interactive REPL for queries and exploration

cortex node create                   Create a node interactively or from flags
cortex node get <id>                 Get a node by ID
cortex node list [--kind X]          List/filter nodes
cortex node delete <id>              Soft-delete a node

cortex edge create                   Create an edge
cortex edge list [--from X]          List edges

cortex search <query>                Semantic similarity search
cortex search --hybrid <query>       Hybrid search (vector + graph)
cortex traverse <id> [--depth N]     Graph traversal from a node
cortex path <from> <to>              Find shortest path between two nodes

cortex briefing <agent_id>           Generate and print a briefing
cortex briefing --compact            Compact mode (shorter output)

cortex import <file>                 Bulk import (JSON, CSV, markdown, JSONL)
cortex export [--format json]        Export graph (JSON, DOT, GraphML, JSONL)
cortex backup <path>                 Verified backup with optional encryption
cortex restore <path>                Restore from backup

cortex migrate                       Run schema migrations after upgrade

cortex stats                         Graph health overview (nodes, edges, kinds, size)
cortex doctor                        Diagnose issues (corrupt index, orphans, stale embeddings)

cortex config validate               Validate cortex.toml
cortex config show                   Show resolved config (with defaults filled in)
```

---

## Task 1: Clap Subcommand Structure

### File: `crates/cortex-server/src/cli/mod.rs`

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cortex")]
#[command(version, about = "Embedded graph memory for AI agents")]
pub struct Cli {
    /// Path to cortex.toml (default: ./cortex.toml)
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
    /// Graph traversal
    Traverse(TraverseArgs),
    /// Find path between nodes
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
```

### File: `crates/cortex-server/src/main.rs`

```rust
use clap::Parser;
use cli::{Cli, Commands};

mod cli;
// ... other modules

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = CortexConfig::load_or_default(&cli.config);

    match cli.command {
        Commands::Serve      => serve::run(config).await?,
        Commands::Init       => cli::init::run().await?,
        Commands::Shell      => cli::shell::run(config, &cli.server).await?,
        Commands::Node(cmd)  => cli::node::run(cmd, &cli.server).await?,
        Commands::Edge(cmd)  => cli::edge::run(cmd, &cli.server).await?,
        Commands::Search(a)  => cli::search::run(a, &cli.server).await?,
        Commands::Traverse(a)=> cli::traverse::run(a, &cli.server).await?,
        Commands::Path(a)    => cli::traverse::run_path(a, &cli.server).await?,
        Commands::Briefing(a)=> cli::briefing::run(a, &cli.server).await?,
        Commands::Import(a)  => cli::import::run(a, config).await?,
        Commands::Export(a)  => cli::export::run(a, &cli.server).await?,
        Commands::Backup(a)  => cli::backup::run(a, config).await?,
        Commands::Restore(a) => cli::backup::run_restore(a, config).await?,
        Commands::Migrate    => cli::migrate::run(config).await?,
        Commands::Stats      => cli::stats::run(&cli.server).await?,
        Commands::Doctor     => cli::doctor::run(config, &cli.server).await?,
        Commands::Config(cmd)=> cli::config_cmd::run(cmd, &cli.config).await?,
    }

    Ok(())
}
```

---

## Task 2: Setup Wizard (`cortex init`)

### File: `crates/cortex-server/src/cli/init.rs`

Interactive walkthrough that generates a `cortex.toml`. Uses the `inquire` crate for prompts.

**Cargo.toml dependency:**
```toml
inquire = "0.7"
```

**Wizard flow:**

```
$ cortex init

Welcome to Cortex — graph memory for AI agents.

? Where should Cortex store data? [./data]
? Which embedding model? (use arrow keys)
  > BAAI/bge-small-en-v1.5 (384d, fast, English)
    BAAI/bge-base-en-v1.5 (768d, balanced)
    BAAI/bge-large-en-v1.5 (1024d, accurate)
    Custom (bring your own via API)
? Enable auto-linker? [Y/n]
? Auto-linker interval (seconds)? [60]
? Enable event ingest? (use arrow keys)
    None
  > File watcher
    Webhook endpoint
    NATS
    All of the above
? Pre-configure agent briefings? Enter agent IDs (comma-separated): [default]
? Enable HTTP debug server? [Y/n]

✅ Generated cortex.toml
✅ Created data directory
✅ Downloading embedding model... done (33MB)

Run `cortex serve` to start, or `cortex shell` for interactive mode.
```

**Implementation:**
```rust
pub async fn run() -> anyhow::Result<()> {
    use inquire::{Confirm, Select, Text};

    println!("\nWelcome to Cortex — graph memory for AI agents.\n");

    let data_dir = Text::new("Where should Cortex store data?")
        .with_default("./data")
        .prompt()?;

    let embedding_model = Select::new(
        "Which embedding model?",
        vec![
            "BAAI/bge-small-en-v1.5 (384d, fast, English)",
            "BAAI/bge-base-en-v1.5 (768d, balanced)",
            "BAAI/bge-large-en-v1.5 (1024d, accurate)",
            "Custom (bring your own via API)",
        ],
    ).prompt()?;

    let autolinker = Confirm::new("Enable auto-linker?")
        .with_default(true)
        .prompt()?;

    let autolinker_interval = if autolinker {
        Text::new("Auto-linker interval (seconds)?")
            .with_default("60")
            .prompt()?
            .parse::<u64>().unwrap_or(60)
    } else { 60 };

    let ingest = Select::new(
        "Enable event ingest?",
        vec!["None", "File watcher", "Webhook endpoint", "NATS", "All of the above"],
    ).prompt()?;

    let agents_str = Text::new("Pre-configure agent briefings? Enter agent IDs (comma-separated):")
        .with_default("default")
        .prompt()?;
    let agents: Vec<String> = agents_str.split(',').map(|s| s.trim().to_string()).collect();

    let http_debug = Confirm::new("Enable HTTP debug server?")
        .with_default(true)
        .prompt()?;

    // Build config
    let config = build_config(
        &data_dir, embedding_model, autolinker, autolinker_interval,
        ingest, &agents, http_debug,
    );

    // Write cortex.toml
    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write("cortex.toml", &toml_str)?;
    println!("\n✅ Generated cortex.toml");

    // Create data directory
    std::fs::create_dir_all(&data_dir)?;
    println!("✅ Created data directory");

    // Note: embedding model download happens on first `cortex serve`
    println!("✅ Ready\n");
    println!("Run `cortex serve` to start, or `cortex shell` for interactive mode.");

    Ok(())
}
```

---

## Task 3: Node Commands

### File: `crates/cortex-server/src/cli/node.rs`

Commands operate against the running server via gRPC (using `cortex-client` from Phase 7C, or raw tonic calls for now).

```rust
#[derive(Args, Debug)]
pub struct NodeCreateArgs {
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long)]
    pub importance: Option<f32>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    /// Read body from stdin
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Args, Debug)]
pub struct NodeGetArgs {
    pub id: String,
}

#[derive(Args, Debug)]
pub struct NodeListArgs {
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long, default_value = "20")]
    pub limit: usize,
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Args, Debug)]
pub struct NodeDeleteArgs {
    pub id: String,
    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

pub async fn run(cmd: NodeCommands, server: &str) -> anyhow::Result<()> {
    match cmd {
        NodeCommands::Create(args) => create(args, server).await,
        NodeCommands::Get(args)    => get(args, server).await,
        NodeCommands::List(args)   => list(args, server).await,
        NodeCommands::Delete(args) => delete(args, server).await,
    }
}
```

Output format: human-readable table by default; `--json` flag for JSON output (machine-readable).

---

## Task 4: Search & Traversal Commands

### File: `crates/cortex-server/src/cli/search.rs`

```rust
#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value = "10")]
    pub limit: usize,
    /// Hybrid search (vector + graph)
    #[arg(long)]
    pub hybrid: bool,
    /// Output format: table (default), json
    #[arg(long, default_value = "table")]
    pub format: String,
}

pub async fn run(args: SearchArgs, server: &str) -> anyhow::Result<()> {
    // Call gRPC SearchNodes or HybridSearch
    // Print results as table: rank | score | kind | title | id
    Ok(())
}
```

### File: `crates/cortex-server/src/cli/traverse.rs`

```rust
#[derive(Args, Debug)]
pub struct TraverseArgs {
    pub id: String,
    #[arg(long, default_value = "2")]
    pub depth: u32,
    /// "outgoing" | "incoming" | "both"
    #[arg(long, default_value = "both")]
    pub direction: String,
    /// Filter by relation type
    #[arg(long)]
    pub relation: Option<String>,
}

#[derive(Args, Debug)]
pub struct PathArgs {
    pub from: String,
    pub to: String,
    #[arg(long, default_value = "5")]
    pub max_hops: u32,
}
```

---

## Task 5: Briefing Command

### File: `crates/cortex-server/src/cli/briefing.rs`

```rust
#[derive(Args, Debug)]
pub struct BriefingArgs {
    pub agent_id: String,
    /// Compact mode (shorter output)
    #[arg(long)]
    pub compact: bool,
    /// Output format: text (default), json, markdown
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Force regenerate (bypass cache)
    #[arg(long)]
    pub no_cache: bool,
}

pub async fn run(args: BriefingArgs, server: &str) -> anyhow::Result<()> {
    // Call gRPC GetBriefing
    // Print the markdown text to stdout
    // In compact mode, truncate and summarise
    Ok(())
}
```

---

## Task 6: Import & Export Commands

### File: `crates/cortex-server/src/cli/import.rs`

```rust
#[derive(Args, Debug)]
pub struct ImportArgs {
    pub file: PathBuf,
    /// "json" | "csv" | "jsonl" | "markdown" | "obsidian" | "notion"
    /// Auto-detected from file extension if not specified.
    #[arg(long)]
    pub format: Option<String>,
    /// Agent ID to assign as source
    #[arg(long, default_value = "import")]
    pub source: String,
    /// Dry run — validate without writing
    #[arg(long)]
    pub dry_run: bool,
}
```

Format auto-detection rules:
- `.json` → JSON array of node objects
- `.jsonl` or `.ndjson` → JSONL (one node per line)
- `.csv` → CSV (columns: kind, title, body, tags)
- `.md` or `.markdown` → Markdown file
- Directory → Obsidian vault (if `--format obsidian`) or Notion export

### File: `crates/cortex-server/src/cli/export.rs`

```rust
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Output file (stdout if not specified)
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// "json" | "jsonl" | "dot" | "graphml"
    #[arg(long, default_value = "json")]
    pub format: String,
    /// Filter by kind
    #[arg(long)]
    pub kind: Option<String>,
}
```

---

## Task 7: Backup & Restore

### File: `crates/cortex-server/src/cli/backup.rs`

Backup creates a point-in-time copy of the redb file with an integrity checksum.

```rust
#[derive(Args, Debug)]
pub struct BackupArgs {
    /// Destination path
    pub path: PathBuf,
    /// Encrypt the backup (requires CORTEX_ENCRYPTION_KEY env var)
    #[arg(long)]
    pub encrypt: bool,
}

#[derive(Args, Debug)]
pub struct RestoreArgs {
    /// Source backup path
    pub path: PathBuf,
    /// Skip confirmation
    #[arg(long)]
    pub yes: bool,
}

pub async fn run(args: BackupArgs, config: CortexConfig) -> anyhow::Result<()> {
    let db_path = config.server.data_dir.join("cortex.redb");

    println!("Creating backup of {} → {}", db_path.display(), args.path.display());

    // 1. Flush (if server is running, send SIGUSR1 or use API)
    // 2. Copy the redb file
    std::fs::copy(&db_path, &args.path)?;

    // 3. Write SHA-256 checksum sidecar file
    let checksum = sha256_file(&args.path)?;
    let checksum_path = args.path.with_extension("sha256");
    std::fs::write(&checksum_path, &checksum)?;

    // 4. Encrypt if requested
    if args.encrypt {
        encrypt_file(&args.path)?;
    }

    println!("✅ Backup complete: {}", args.path.display());
    println!("   Checksum: {}", checksum);
    Ok(())
}

pub async fn run_restore(args: RestoreArgs, config: CortexConfig) -> anyhow::Result<()> {
    // 1. Verify checksum
    // 2. Confirm with user (unless --yes)
    // 3. Stop server if running (or warn)
    // 4. Replace db file
    // 5. Run migrations if needed
    Ok(())
}
```

---

## Task 8: Doctor & Stats

### File: `crates/cortex-server/src/cli/stats.rs`

```
$ cortex stats

Graph Overview
──────────────────────────────
Nodes:   12,459  (fact: 4,231  event: 3,102  decision: 891  ...)
Edges:   34,781  (related_to: 21,234  informed_by: 8,231  ...)
DB Size: 47.2 MB
Index:   23.1 MB (HNSW)
Schema:  v2
──────────────────────────────
Auto-linker: last run 42s ago, created 12 edges, pruned 3
Briefing cache: 3 agents cached, oldest 4m12s
```

### File: `crates/cortex-server/src/cli/doctor.rs`

```
$ cortex doctor

Cortex Health Check
──────────────────────────────
[✓] Database file accessible: ./data/cortex.redb
[✓] Schema version: v2 (current)
[✓] Embedding index: 12,459 vectors, matches node count
[✗] Orphaned edges: 14 edges reference deleted nodes
    → Run `cortex doctor --fix` to prune orphaned edges
[✓] No corrupt nodes detected
[✓] Auto-linker: running (last cycle 42s ago)
[⚠] 234 nodes have no embedding (embedding service was unavailable)
    → Run `cortex doctor --reembed` to backfill embeddings
```

```rust
pub async fn run(config: CortexConfig, server: &str) -> anyhow::Result<()> {
    // Each check is a function returning CheckResult { name, status, detail, fix }
    let checks = vec![
        check_db_accessible(&config),
        check_schema_version(&config),
        check_index_sync(server).await,
        check_orphaned_edges(server).await,
        check_missing_embeddings(server).await,
    ];
    // Print formatted results
    Ok(())
}
```

---

## Task 9: Migrate Command

### File: `crates/cortex-server/src/cli/migrate.rs`

```rust
pub async fn run(config: CortexConfig) -> anyhow::Result<()> {
    let db_path = config.server.data_dir.join("cortex.redb");

    println!("Cortex data at {}", db_path.display());

    let current_version = read_schema_version(&db_path)?;
    let target_version = cortex_core::CURRENT_SCHEMA_VERSION;

    println!("Current schema: v{}", current_version);
    println!("Target schema:  v{}", target_version);

    if current_version == target_version {
        println!("Already up to date.");
        return Ok(());
    }

    // List migrations to apply
    let migrations = migrations_to_apply(current_version, target_version);
    println!("\nMigrations to apply:");
    for m in &migrations {
        println!("  v{} → v{}: {}", m.from, m.to, m.description);
    }

    // Backup first
    let backup_path = db_path.with_extension(format!("redb.v{}.bak", current_version));
    println!("\nCreating backup at {}...", backup_path.display());
    std::fs::copy(&db_path, &backup_path)?;
    println!("done");

    // Apply migrations
    for m in migrations {
        print!("Applying v{} → v{}...", m.from, m.to);
        let start = std::time::Instant::now();
        (m.apply)(&db_path)?;
        println!(" done ({:.1}s)", start.elapsed().as_secs_f32());
    }

    println!("\nSchema upgraded to v{}.", target_version);
    Ok(())
}
```

---

## Task 10: Config Commands

### File: `crates/cortex-server/src/cli/config_cmd.rs`

```rust
pub async fn run(cmd: ConfigCommands, config_path: &Path) -> anyhow::Result<()> {
    match cmd {
        ConfigCommands::Validate => {
            let config = CortexConfig::load(config_path)?;
            let errors = config.validate();
            if errors.is_empty() {
                println!("✅ cortex.toml is valid.");
            } else {
                println!("❌ Validation errors:");
                for e in errors { println!("  - {}", e); }
                std::process::exit(1);
            }
        }
        ConfigCommands::Show => {
            let config = CortexConfig::load_or_default(config_path);
            // Print as TOML with defaults filled in
            println!("{}", toml::to_string_pretty(&config)?);
        }
    }
    Ok(())
}
```

---

## Task 11: Library Mode API

### File: `crates/cortex-core/src/api.rs` (new file)

The `Cortex::open()` API gives users a single struct for embedded, no-server usage.

```rust
use crate::{
    Config, NodeKind, Relation, Node, Edge, NodeId, Source, Result,
    RedbStorage, FastEmbedService, HnswIndex, AutoLinker, AutoLinkerConfig,
    HybridSearch, Storage, NodeFilter,
};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::sync::atomic::AtomicU64;

/// High-level, embedded Cortex API.
/// No server required — runs everything in-process.
///
/// # Example
/// ```rust
/// use cortex_core::{Cortex, Config};
///
/// let cortex = Cortex::open("./memory.redb", Config::default())?;
///
/// cortex.store(Node::fact("The API uses JWT auth", 0.7))?;
/// let results = cortex.search("authentication", 5)?;
/// let briefing = cortex.briefing("my-agent")?;
/// ```
pub struct Cortex {
    storage: Arc<RedbStorage>,
    embedding: Arc<FastEmbedService>,
    index: Arc<RwLock<HnswIndex>>,
    config: Config,
}

/// Config for library mode. Maps to a subset of CortexConfig.
#[derive(Debug, Clone)]
pub struct Config {
    pub embedding_model: String,
    pub auto_linker: AutoLinkerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            embedding_model: "BAAI/bge-small-en-v1.5".into(),
            auto_linker: AutoLinkerConfig::new(),
        }
    }
}

impl Cortex {
    /// Open (or create) a Cortex database at the given path.
    /// Equivalent to SQLite's `Connection::open()`.
    pub fn open(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        let storage = Arc::new(RedbStorage::open(path.as_ref())?);
        let embedding = Arc::new(FastEmbedService::new(&config.embedding_model)?);

        // Load existing nodes into HNSW index
        let index = {
            let mut idx = HnswIndex::new(embedding.dimension());
            let nodes = storage.list_nodes(NodeFilter::new())?;
            for node in &nodes {
                if let Some(emb) = &node.embedding {
                    idx.insert(node.id, emb)?;
                }
            }
            Arc::new(RwLock::new(idx))
        };

        Ok(Self { storage, embedding, index, config })
    }

    /// Store a node. Generates embedding automatically.
    pub fn store(&self, mut node: Node) -> Result<NodeId> {
        if node.embedding.is_none() {
            let text = crate::vector::embedding_input(&node);
            node.embedding = Some(self.embedding.embed(&text)?);
        }
        let id = node.id;
        let emb = node.embedding.clone().unwrap();
        self.storage.put_node(&node)?;
        self.index.write().unwrap().insert(id, &emb)?;
        Ok(id)
    }

    /// Semantic similarity search. Returns nodes ranked by score.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(f32, Node)>> {
        let query_emb = self.embedding.embed(query)?;
        let index = self.index.read().unwrap();
        let results = index.search(&query_emb, limit)?;
        let mut out = Vec::new();
        for r in results {
            if let Some(node) = self.storage.get_node(r.id)? {
                out.push((r.score, node));
            }
        }
        Ok(out)
    }

    /// Hybrid search (vector + graph proximity).
    pub fn search_hybrid(&self, query: &str, limit: usize) -> Result<Vec<(f32, Node)>> {
        // Uses HybridSearch from cortex-core
        todo!()
    }

    /// Generate a context briefing for an agent.
    pub fn briefing(&self, agent_id: &str) -> Result<String> {
        // Uses BriefingEngine from cortex-core
        todo!()
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        self.storage.get_node(id)
    }

    /// List nodes with optional filter.
    pub fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>> {
        self.storage.list_nodes(filter)
    }

    /// Create an edge between two nodes.
    pub fn create_edge(&self, edge: Edge) -> Result<()> {
        self.storage.put_edge(&edge)
    }

    /// Graph traversal from a node.
    pub fn traverse(&self, from: NodeId, depth: u32) -> Result<crate::graph::Subgraph> {
        todo!()
    }

    /// Convenience constructor for common node types.
    pub fn fact(title: &str, importance: f32) -> Node {
        Node::new(
            NodeKind::new("fact").unwrap(),
            title.into(),
            title.into(),
            Source { agent: "library".into(), session: None, channel: None },
            importance,
        )
    }
}
```

**Node convenience constructors** (so users don't need to import `NodeKind::new`):

```rust
impl Node {
    pub fn fact(title: &str, importance: f32) -> Self {
        Self::new(NodeKind::new("fact").unwrap(), title.into(), title.into(),
            Source { agent: "library".into(), session: None, channel: None }, importance)
    }
    pub fn decision(title: &str, body: &str, importance: f32) -> Self {
        Self::new(NodeKind::new("decision").unwrap(), title.into(), body.into(),
            Source { agent: "library".into(), session: None, channel: None }, importance)
    }
    // ... similarly for event, goal, observation, pattern, preference, agent
}
```

**Export from `crates/cortex-core/src/lib.rs`:**
```rust
pub mod api;
pub use api::{Cortex, Config as LibraryConfig};
```

---

## Task 12: Shell REPL

### File: `crates/cortex-server/src/cli/shell.rs`

Interactive REPL for querying and exploring the graph. Uses `rustyline` for readline support.

**Cargo.toml dependency:**
```toml
rustyline = "14"
```

```
$ cortex shell

Cortex shell — connected to localhost:9090
Type 'help' for available commands, 'exit' to quit.

cortex> search "FastAPI vs Flask"
 1. 0.92  decision  Use FastAPI for the backend   [id: abc123]
 2. 0.87  fact      FastAPI supports async out of the box  [id: def456]

cortex> node get abc123
Kind:       decision
Title:      Use FastAPI for the backend
Body:       Chose FastAPI over Flask for async support and type hints
Importance: 0.80
Tags:       backend, python
Created:    2026-01-15T10:23:41Z

cortex> traverse abc123 --depth 2
[graph display]

cortex> briefing my-agent
[briefing text]

cortex> exit
```

**Commands available in shell:** All CLI commands available as REPL commands without the `cortex` prefix.

---

## Definition of Done

- [ ] `cortex --help` shows all subcommands
- [ ] `cortex init` runs interactively and writes a valid `cortex.toml`
- [ ] `cortex config validate` passes on a valid `cortex.toml` and exits 1 on invalid
- [ ] `cortex config show` prints the resolved config including defaults
- [ ] `cortex node create --kind fact --title "Test" --body "Body"` creates a node (requires running server)
- [ ] `cortex node list --kind fact` returns facts only
- [ ] `cortex node get <id>` returns node detail
- [ ] `cortex node delete <id>` soft-deletes a node with confirmation
- [ ] `cortex edge create` creates an edge
- [ ] `cortex search "query"` returns ranked results
- [ ] `cortex search --hybrid "query"` returns hybrid results
- [ ] `cortex traverse <id> --depth 2` returns a subgraph
- [ ] `cortex path <from> <to>` returns shortest path
- [ ] `cortex briefing <agent_id>` prints a briefing to stdout
- [ ] `cortex briefing --compact <agent_id>` prints a shorter briefing
- [ ] `cortex import data.json` imports nodes from a JSON file
- [ ] `cortex import data.csv` imports nodes from a CSV file
- [ ] `cortex import data.jsonl` imports nodes from JSONL
- [ ] `cortex export --format json` exports graph to JSON
- [ ] `cortex export --format dot` exports graph in DOT format
- [ ] `cortex backup ./backup.redb` creates a backup with SHA-256 sidecar
- [ ] `cortex restore ./backup.redb` restores from backup
- [ ] `cortex migrate` prints the migration plan and applies it
- [ ] `cortex stats` prints node/edge counts by kind/relation plus DB size
- [ ] `cortex doctor` detects orphaned edges and missing embeddings
- [ ] `cortex shell` starts an interactive REPL with readline support
- [ ] `Cortex::open("./memory.redb", Config::default())` works without a running server
- [ ] `cortex.store(Node::fact("test", 0.5))` stores a node in library mode
- [ ] `cortex.search("test", 5)` returns matching nodes in library mode
- [ ] `cortex.briefing("agent")` returns a briefing string in library mode
- [ ] All commands accept `--json` for machine-readable output
- [ ] `cargo test --workspace` passes
