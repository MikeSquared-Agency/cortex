# Phase B — CLI & Library Mode

**Duration:** 2 weeks  
**Dependencies:** Phase A complete  
**Goal:** Make Cortex usable without writing Rust. CLI for humans, library mode for embedding.

---

## B1. CLI Framework

### Architecture

Single `cortex` binary serves as both server and CLI tool. Uses `clap` with subcommands.

```rust
#[derive(Parser)]
#[command(name = "cortex", about = "Graph memory for AI agents")]
pub struct Cli {
    /// Path to config file
    #[arg(long, default_value = "cortex.toml")]
    pub config: PathBuf,
    
    /// Data directory (overrides config)
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the gRPC + HTTP server
    Serve(ServeArgs),
    /// Interactive setup wizard
    Init(InitArgs),
    /// Interactive query shell
    Shell,
    /// Graph health overview
    Stats,
    /// Diagnose issues
    Doctor,
    /// Node operations
    Node(NodeCommands),
    /// Edge operations
    Edge(EdgeCommands),
    /// Semantic search
    Search(SearchArgs),
    /// Graph traversal
    Traverse(TraverseArgs),
    /// Find shortest path
    Path(PathArgs),
    /// Generate briefing
    Briefing(BriefingArgs),
    /// Import data
    Import(ImportArgs),
    /// Export graph
    Export(ExportArgs),
    /// Backup database
    Backup(BackupArgs),
    /// Restore from backup
    Restore(RestoreArgs),
    /// Run schema migrations
    Migrate,
    /// Configuration management
    Config(ConfigCommands),
    /// View audit log
    Audit(AuditArgs),
}
```

### Output Formatting

All commands support `--format` flag:
- `table` (default for TTY) — coloured, aligned, human-readable
- `json` — machine-readable, one JSON object per result
- `jsonl` — JSON Lines for streaming/piping
- `csv` — for spreadsheet import
- `quiet` — IDs only (for scripting: `cortex search "X" -q | xargs cortex node get`)

```rust
pub enum OutputFormat {
    Table,
    Json,
    JsonLines,
    Csv,
    Quiet,
}
```

Detection: if stdout is a TTY → table. If piped → json. Override with `--format`.

---

## B2. Setup Wizard (`cortex init`)

Interactive prompts using `dialoguer` crate.

```rust
pub fn run_init(args: InitArgs) -> Result<()> {
    println!("Welcome to Cortex — graph memory for AI agents.\n");
    
    let data_dir: String = Input::new()
        .with_prompt("Where should Cortex store data?")
        .default("./data".into())
        .interact_text()?;
    
    let model = Select::new()
        .with_prompt("Which embedding model?")
        .items(&[
            "BAAI/bge-small-en-v1.5 (384d, fast, English)",
            "BAAI/bge-base-en-v1.5 (768d, balanced)",
            "BAAI/bge-large-en-v1.5 (1024d, accurate)",
            "Custom (bring your own via API)",
        ])
        .default(0)
        .interact()?;
    
    let auto_linker = Confirm::new()
        .with_prompt("Enable auto-linker?")
        .default(true)
        .interact()?;
    
    let interval: u64 = if auto_linker {
        Input::new()
            .with_prompt("Auto-linker interval (seconds)")
            .default(60)
            .interact_text()?
    } else { 60 };
    
    let ingest = MultiSelect::new()
        .with_prompt("Enable event ingest adapters")
        .items(&["File watcher", "Webhook endpoint", "NATS", "Stdin"])
        .interact()?;
    
    let agents: String = Input::new()
        .with_prompt("Pre-configure agent briefings (comma-separated IDs)")
        .default("default".into())
        .interact_text()?;
    
    let http_debug = Confirm::new()
        .with_prompt("Enable HTTP debug server?")
        .default(true)
        .interact()?;
    
    // Generate cortex.toml
    let config = generate_config(data_dir, model, auto_linker, interval, ingest, agents, http_debug);
    std::fs::write("cortex.toml", config)?;
    
    // Create data directory
    std::fs::create_dir_all(&data_dir)?;
    
    // Download embedding model
    println!("\n✅ Generated cortex.toml");
    println!("✅ Created data directory");
    print!("✅ Downloading embedding model... ");
    download_model(model)?;
    println!("done");
    
    println!("\nRun `cortex serve` to start, or `cortex shell` for interactive mode.");
    Ok(())
}
```

Non-interactive mode: `cortex init --defaults` generates config with all defaults, no prompts.

---

## B3. Interactive Shell (`cortex shell`)

REPL using `rustyline` crate with syntax highlighting and tab completion.

```
$ cortex shell
Cortex v0.2.0 — type 'help' for commands, 'exit' to quit

cortex> stats
Nodes: 1,247 (fact: 523, decision: 201, event: 189, pattern: 98, ...)
Edges: 4,891 (related-to: 2,103, led-to: 891, ...)
DB size: 12.4 MB
Auto-linker: running (last cycle: 2s ago, 3 edges created)

cortex> search "database technology"
  0.92  [decision] Use Rust for Cortex — CPU-bound workload justifies complexity
  0.87  [fact] redb is an embedded ACID key-value store
  0.81  [decision] Rejected Neo4j — wanted custom built
  0.74  [pattern] Workers without integration instructions miss wiring

cortex> node get 018d5f2a-...
Kind: decision
Title: Use Rust for Cortex
Body: Chose Rust over Go for the graph engine due to CPU-bound...
Tags: [cortex, rust, architecture]
Importance: 0.8
Edges: 7 outgoing, 3 incoming
Created: 2026-02-18 12:00 UTC

cortex> traverse 018d5f2a-... --depth 2 --direction outgoing
[graph visualisation in terminal using box-drawing characters]

cortex> briefing kai
# Briefing: kai
_Generated: 2026-02-19 12:00 UTC_

## Identity
- **Kai**: Orchestrator agent, Opus 4.6...
...

cortex> .export json > backup.json
Exported 1,247 nodes and 4,891 edges.

cortex> exit
```

**Tab completion:** command names, node IDs (prefix match), kind names, relation names.
**History:** saved to `~/.cortex_history`, searchable with Ctrl+R.
**Piping:** `cortex shell -c "search 'X' | head 3"` for one-shot commands.

---

## B4. Library Mode

The SQLite model — embed Cortex directly without running a server.

### Public API

```rust
/// High-level convenience API for embedding Cortex in applications.
pub struct Cortex {
    storage: Arc<RedbStorage>,
    graph: Arc<GraphEngineImpl<RedbStorage>>,
    vectors: Arc<RwLock<HnswIndex>>,
    embeddings: Arc<FastEmbedService>,
    briefing: Arc<BriefingEngine<...>>,
    auto_linker: Option<AutoLinkerHandle>,
    config: CortexConfig,
}

impl Cortex {
    /// Open or create a Cortex database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_config(path, CortexConfig::default())
    }
    
    /// Open with custom configuration.
    pub fn open_with_config(path: impl AsRef<Path>, config: CortexConfig) -> Result<Self> {
        // Initialize storage, embeddings, vector index, graph engine
        // Run schema migration if needed
        // Optionally start auto-linker background thread
    }
    
    /// Store a new knowledge node.
    pub fn store(&self, kind: &str, title: &str) -> NodeBuilder {
        NodeBuilder::new(self, kind, title)
    }
    
    /// Search by semantic similarity.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> { ... }
    
    /// Hybrid search (vector + graph proximity).
    pub fn hybrid_search(&self, query: &str) -> HybridSearchBuilder { ... }
    
    /// Generate a briefing for an agent.
    pub fn briefing(&self, agent_id: &str) -> Result<Briefing> { ... }
    
    /// Traverse the graph from a starting node.
    pub fn traverse(&self, start: NodeId) -> TraversalBuilder { ... }
    
    /// Find shortest path between two nodes.
    pub fn path(&self, from: NodeId, to: NodeId) -> Result<Option<Path>> { ... }
    
    /// Get a node by ID.
    pub fn get(&self, id: NodeId) -> Result<Option<Node>> { ... }
    
    /// Get graph statistics.
    pub fn stats(&self) -> Result<Stats> { ... }
    
    /// Graceful shutdown (flushes vector index, stops auto-linker).
    pub fn close(self) -> Result<()> { ... }
}
```

### Builder Pattern

```rust
// Fluent API for node creation
let node = cortex.store("decision", "Use Cortex for memory")
    .body("Embedded graph beats external database for our use case")
    .tags(&["architecture", "memory"])
    .importance(0.8)
    .source("kai")
    .commit()?;

// Fluent API for hybrid search
let results = cortex.hybrid_search("infrastructure decisions")
    .anchors(&[agent_node_id])
    .vector_weight(0.7)
    .limit(10)
    .execute()?;

// Fluent API for traversal
let subgraph = cortex.traverse(node_id)
    .depth(3)
    .direction(Direction::Outgoing)
    .relations(&["led-to", "informed-by"])
    .min_weight(0.3)
    .execute()?;
```

### Auto-Linker in Library Mode

Runs as a background thread (not tokio — library users may not use async):

```rust
pub struct AutoLinkerHandle {
    thread: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl Cortex {
    pub fn open_with_config(path: &Path, config: CortexConfig) -> Result<Self> {
        // ...
        let auto_linker = if config.auto_linker.enabled {
            Some(AutoLinkerHandle::spawn(storage.clone(), ...))
        } else {
            None
        };
    }
}
```

---

## B5. Backup & Restore

### Backup

```rust
pub fn backup(db_path: &Path, backup_path: &Path, encrypt: bool) -> Result<BackupInfo> {
    // 1. Create consistent snapshot (redb supports this natively)
    let storage = RedbStorage::open(db_path)?;
    storage.snapshot(backup_path)?;
    
    // 2. Optional encryption
    if encrypt {
        let key = std::env::var("CORTEX_ENCRYPTION_KEY")
            .map_err(|_| CortexError::Validation("CORTEX_ENCRYPTION_KEY not set".into()))?;
        encrypt_file(backup_path, &key)?;
    }
    
    // 3. Compute checksum
    let checksum = sha256_file(backup_path)?;
    
    // 4. Write metadata sidecar
    let info = BackupInfo {
        created_at: Utc::now(),
        schema_version: SCHEMA_VERSION,
        node_count: storage.stats()?.node_count,
        edge_count: storage.stats()?.edge_count,
        checksum,
        encrypted: encrypt,
    };
    let sidecar = format!("{}.meta.json", backup_path.display());
    std::fs::write(&sidecar, serde_json::to_string_pretty(&info)?)?;
    
    Ok(info)
}
```

### Restore

```rust
pub fn restore(backup_path: &Path, target_path: &Path) -> Result<()> {
    // 1. Verify checksum
    let sidecar = format!("{}.meta.json", backup_path.display());
    let info: BackupInfo = serde_json::from_str(&std::fs::read_to_string(&sidecar)?)?;
    let actual_checksum = sha256_file(backup_path)?;
    if actual_checksum != info.checksum {
        return Err(CortexError::Validation("Backup checksum mismatch — file may be corrupted".into()));
    }
    
    // 2. Decrypt if needed
    if info.encrypted {
        let key = std::env::var("CORTEX_ENCRYPTION_KEY")?;
        decrypt_file(backup_path, target_path, &key)?;
    } else {
        std::fs::copy(backup_path, target_path)?;
    }
    
    // 3. Verify restored database opens
    let storage = RedbStorage::open(target_path)?;
    let stats = storage.stats()?;
    println!("Restored: {} nodes, {} edges", stats.node_count, stats.edge_count);
    
    Ok(())
}
```

CLI:
```
cortex backup ./backups/cortex-2026-02-19.bak
cortex backup ./backups/encrypted.bak --encrypt
cortex restore ./backups/cortex-2026-02-19.bak --target ./data/cortex.redb
```

---

## B6. Doctor & Stats

### `cortex stats`

```
$ cortex stats
Cortex v0.2.0 — ./data/cortex.redb

Nodes:     1,247
  agent:        12
  decision:    201
  fact:        523
  event:       189
  pattern:      98
  preference:   45
  observation: 156
  goal:         23

Edges:     4,891
  related-to:  2,103
  led-to:        891
  informed-by:   634
  applies-to:    512
  supersedes:    201
  contradicts:    47
  depends-on:    312
  instance-of:   191

DB size:   12.4 MB
Schema:    v2
Uptime:    4h 23m (if server running)
Auto-linker: 847 cycles, 12,491 edges created, 892 pruned
```

### `cortex doctor`

```
$ cortex doctor
Running diagnostics...

✅ Database opens successfully
✅ Schema version: v2 (current)
✅ All secondary indexes consistent
✅ Vector index: 1,247 vectors (matches node count)
⚠️  23 orphaned edges (endpoints deleted but edges remain)
   Run `cortex doctor --fix` to clean up
⚠️  Vector index stale (47 nodes added since last rebuild)
   Run `cortex doctor --fix` to rebuild
✅ No duplicate edges found
✅ Embedding model loaded: BAAI/bge-small-en-v1.5
✅ Data directory writable

2 warnings found. Run `cortex doctor --fix` to auto-repair.
```

`--fix` flag:
- Removes orphaned edges
- Rebuilds vector index
- Rebuilds secondary indexes if inconsistent
- Removes tombstoned nodes past grace period

---

## B7. Import & Export

### Import

```rust
pub enum ImportFormat {
    Json,      // Array of node objects
    JsonLines, // One node per line
    Csv,       // Headers: kind,title,body,tags,importance
    Markdown,  // Split by headings, classify with heuristics
    Obsidian,  // Markdown vault with [[wikilinks]] → edges
}
```

**Obsidian importer** (the early adoption killer):
```rust
pub fn import_obsidian(vault_path: &Path, storage: &dyn Storage) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    
    for entry in WalkDir::new(vault_path).into_iter().filter_map(|e| e.ok()) {
        if entry.path().extension() == Some("md".as_ref()) {
            let content = std::fs::read_to_string(entry.path())?;
            
            // Create node from file
            let title = entry.path().file_stem().unwrap().to_string_lossy();
            let kind = classify_chunk(&content);
            let node = Node::new(kind, title.to_string(), content.clone(), ...);
            storage.put_node(&node)?;
            stats.nodes += 1;
            
            // Extract [[wikilinks]] and create edges
            let wikilinks = extract_wikilinks(&content);
            for link in wikilinks {
                // Find or create target node
                // Create RelatedTo edge
                stats.edges += 1;
            }
            
            // Extract #tags
            let tags = extract_hashtags(&content);
            // Add as node tags
        }
    }
    
    Ok(stats)
}
```

### Export

```
cortex export --format json > graph.json
cortex export --format dot > graph.dot          # For Graphviz
cortex export --format graphml > graph.graphml   # For Gephi/yEd
cortex export --format jsonl > graph.jsonl       # Streaming
cortex export --nodes-only --format csv > nodes.csv
```

---

## Deliverables

1. Full CLI with all subcommands
2. `cortex init` setup wizard (interactive + `--defaults`)
3. `cortex shell` REPL with completion and history
4. `Cortex::open()` library API with builder pattern
5. Backup/restore with optional encryption and checksum verification
6. `cortex doctor` with `--fix` auto-repair
7. `cortex stats` health overview
8. Import: JSON, JSONL, CSV, Markdown, Obsidian
9. Export: JSON, JSONL, CSV, DOT, GraphML
10. Output formatting: table, json, jsonl, csv, quiet
