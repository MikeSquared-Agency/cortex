# Phase 7D: Import Adapters

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires `IngestAdapter` trait, `CortexConfig`, string `NodeKind`.  
**Weeks:** 9–10 (can run in parallel with 7E/7F/7G)  

---

## Overview

Structured importers for common knowledge sources, plus an upgrade to the existing file watcher to use platform-native filesystem event APIs (inotify on Linux, FSEvents on macOS). The Obsidian vault importer is the highest-priority feature for early adoption — personal knowledge management users have structured vaults that Cortex can turn into live knowledge graphs.

---

## Repository Layout

```
crates/cortex-server/src/
  import/
    mod.rs            — ImportAdapter trait + format detection
    json.rs           — JSON array importer
    csv.rs            — CSV importer
    jsonl.rs          — JSON Lines importer
    markdown.rs       — Markdown file importer
    obsidian.rs       — Obsidian vault importer (wikilinks → edges)
    notion.rs         — Notion HTML/MD export importer
  ingest/
    file_watcher.rs   — upgraded file watcher (inotify/fsevents)
```

---

## CLI Interface

```
cortex import <file>                        Auto-detect format from extension
cortex import --format json data.json       JSON array of nodes
cortex import --format csv facts.csv        CSV (kind,title,body,tags)
cortex import --format jsonl stream.jsonl   JSON Lines
cortex import --format markdown notes.md    Markdown file → single node
cortex import --format obsidian ~/vault/    Obsidian vault directory
cortex import --format notion <export-dir>  Notion HTML/MD export directory
```

All import commands accept:
- `--source <agent_id>` — agent ID to assign as node source (default: `"import"`)
- `--dry-run` — parse and validate without writing to the database
- `--kind <kind>` — override the node kind for all imported nodes (where format doesn't specify)
- `--namespace <ns>` — assign a namespace to all imported nodes (for Phase 7E)

---

## Task 1: ImportAdapter Trait

### File: `crates/cortex-server/src/import/mod.rs`

```rust
use cortex_core::{Node, Edge, Result};
use std::path::Path;

/// A batch of nodes and edges to import.
#[derive(Debug, Default)]
pub struct ImportBatch {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// An adapter that reads from a file/directory and produces an ImportBatch.
pub trait ImportAdapter: Send + Sync {
    /// Parse the source and return all nodes and edges to import.
    fn import(&self, source_path: &Path, options: &ImportOptions) -> Result<ImportBatch>;
}

/// Options passed to all importers.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Agent ID to assign as source.
    pub source_agent: String,
    /// Override kind for all imported nodes. None = use format-specific default.
    pub kind_override: Option<String>,
    /// Dry run — parse but don't write.
    pub dry_run: bool,
    /// Namespace to assign (for Phase 7E).
    pub namespace: Option<String>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            source_agent: "import".into(),
            kind_override: None,
            dry_run: false,
            namespace: None,
        }
    }
}

/// Detect format from file extension or directory structure.
pub fn detect_format(path: &Path) -> Option<&'static str> {
    if path.is_dir() {
        // Check for Obsidian vault markers
        if path.join(".obsidian").exists() {
            return Some("obsidian");
        }
        return None; // Unknown directory
    }
    match path.extension()?.to_str()? {
        "json"  => Some("json"),
        "jsonl" | "ndjson" => Some("jsonl"),
        "csv"   => Some("csv"),
        "md" | "markdown" => Some("markdown"),
        _       => None,
    }
}

/// Run an import, writing to storage.
pub async fn run_import(
    path: &Path,
    format: &str,
    options: ImportOptions,
    storage: &cortex_core::RedbStorage,
    embedding: &cortex_core::FastEmbedService,
    index: &std::sync::RwLock<cortex_core::HnswIndex>,
) -> Result<ImportSummary> {
    let adapter: Box<dyn ImportAdapter> = match format {
        "json"     => Box::new(JsonImporter),
        "jsonl"    => Box::new(JsonlImporter),
        "csv"      => Box::new(CsvImporter),
        "markdown" => Box::new(MarkdownImporter),
        "obsidian" => Box::new(ObsidianImporter),
        "notion"   => Box::new(NotionImporter),
        other      => return Err(cortex_core::CortexError::Validation(
            format!("Unknown import format: {}", other)
        )),
    };

    let batch = adapter.import(path, &options)?;

    if options.dry_run {
        println!("Dry run: {} nodes, {} edges would be imported.", batch.nodes.len(), batch.edges.len());
        return Ok(ImportSummary { nodes_imported: 0, edges_imported: 0, nodes_skipped: 0 });
    }

    let mut imported_nodes = 0;
    let mut imported_edges = 0;
    let mut skipped = 0;

    for mut node in batch.nodes {
        // Generate embedding
        let text = cortex_core::vector::embedding_input(&node);
        if let Ok(emb) = embedding.embed(&text) {
            node.embedding = Some(emb.clone());
            let _ = storage.put_node(&node);
            let _ = index.write().unwrap().insert(node.id, &emb);
            imported_nodes += 1;
        } else {
            skipped += 1;
        }
    }

    for edge in batch.edges {
        let _ = storage.put_edge(&edge);
        imported_edges += 1;
    }

    Ok(ImportSummary { nodes_imported: imported_nodes, edges_imported: imported_edges, nodes_skipped: skipped })
}

#[derive(Debug)]
pub struct ImportSummary {
    pub nodes_imported: usize,
    pub edges_imported: usize,
    pub nodes_skipped: usize,
}
```

---

## Task 2: JSON Importer

### File: `crates/cortex-server/src/import/json.rs`

Imports a JSON array of node objects.

**Expected format:**
```json
[
  {
    "kind": "fact",
    "title": "The API uses JWT auth",
    "body": "JWT tokens are validated using RS256.",
    "tags": ["api", "security"],
    "importance": 0.7
  },
  {
    "kind": "decision",
    "title": "Use PostgreSQL for the main database",
    "body": "Chosen for ACID compliance and full-text search.",
    "importance": 0.9
  }
]
```

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, NodeKind, Source, Result, CortexError};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct JsonNode {
    kind: Option<String>,
    title: String,
    body: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f32>,
}

pub struct JsonImporter;

impl ImportAdapter for JsonImporter {
    fn import(&self, path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| CortexError::Validation(format!("Cannot read {}: {}", path.display(), e)))?;

        let raw: Vec<JsonNode> = serde_json::from_str(&content)
            .map_err(|e| CortexError::Validation(format!("Invalid JSON: {}", e)))?;

        let mut batch = ImportBatch::default();
        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("import".into()),
        };

        for item in raw {
            let kind_str = options.kind_override.clone()
                .or(item.kind)
                .unwrap_or_else(|| "fact".into());
            let kind = NodeKind::new(&kind_str)?;

            let node = Node::new(
                kind,
                item.title.clone(),
                item.body.unwrap_or_else(|| item.title.clone()),
                source.clone(),
                item.importance.unwrap_or(0.5),
            );
            // Note: tags set via node.data.tags after construction
            batch.nodes.push(node);
        }

        Ok(batch)
    }
}
```

---

## Task 3: CSV Importer

### File: `crates/cortex-server/src/import/csv.rs`

**Expected format (columns: kind, title, body, tags, importance):**
```csv
kind,title,body,tags,importance
fact,"API rate limit","1000 requests per minute","api,limits",0.7
decision,"Use Redis for caching","Low latency session storage","redis,cache",0.8
```

Tags column is a comma-separated list within the field.

**Cargo.toml dependency:**
```toml
csv = "1.3"
```

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, NodeKind, Source, Result, CortexError};
use std::path::Path;

pub struct CsvImporter;

impl ImportAdapter for CsvImporter {
    fn import(&self, path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let mut reader = csv::Reader::from_path(path)
            .map_err(|e| CortexError::Validation(format!("Cannot read CSV: {}", e)))?;

        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("import".into()),
        };

        let mut batch = ImportBatch::default();

        for result in reader.records() {
            let record = result
                .map_err(|e| CortexError::Validation(format!("CSV parse error: {}", e)))?;

            let kind_str = options.kind_override.clone()
                .or_else(|| record.get(0).map(|s| s.to_string()))
                .unwrap_or_else(|| "fact".into());
            let kind = NodeKind::new(&kind_str)?;

            let title = record.get(1).unwrap_or("").to_string();
            let body = record.get(2).unwrap_or("").to_string();
            let tags: Vec<String> = record.get(3)
                .unwrap_or("")
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            let importance: f32 = record.get(4)
                .unwrap_or("0.5")
                .parse()
                .unwrap_or(0.5);

            let mut node = Node::new(kind, title, body, source.clone(), importance);
            node.data.tags = tags;
            batch.nodes.push(node);
        }

        Ok(batch)
    }
}
```

---

## Task 4: JSONL Importer

### File: `crates/cortex-server/src/import/jsonl.rs`

One JSON object per line. Streaming-friendly for large imports.

**Expected format:**
```jsonl
{"kind":"fact","title":"NATS runs on port 4222","importance":0.6}
{"kind":"event","title":"Deployed v2.1","body":"Production deployment successful","tags":["deploy"]}
```

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, NodeKind, Source, Result, CortexError};
use serde::Deserialize;
use std::path::Path;
use std::io::{BufRead, BufReader};

#[derive(Deserialize)]
struct JsonlNode {
    kind: Option<String>,
    title: String,
    body: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f32>,
}

pub struct JsonlImporter;

impl ImportAdapter for JsonlImporter {
    fn import(&self, path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let file = std::fs::File::open(path)
            .map_err(|e| CortexError::Validation(format!("Cannot open {}: {}", path.display(), e)))?;
        let reader = BufReader::new(file);

        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("import".into()),
        };

        let mut batch = ImportBatch::default();

        for (line_no, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| CortexError::Validation(format!("IO error line {}: {}", line_no + 1, e)))?;
            if line.trim().is_empty() { continue; }

            let item: JsonlNode = serde_json::from_str(&line)
                .map_err(|e| CortexError::Validation(format!("Invalid JSON on line {}: {}", line_no + 1, e)))?;

            let kind_str = options.kind_override.clone()
                .or(item.kind)
                .unwrap_or_else(|| "fact".into());
            let kind = NodeKind::new(&kind_str)?;

            let mut node = Node::new(
                kind,
                item.title.clone(),
                item.body.unwrap_or_else(|| item.title.clone()),
                source.clone(),
                item.importance.unwrap_or(0.5),
            );
            node.data.tags = item.tags.unwrap_or_default();
            batch.nodes.push(node);
        }

        Ok(batch)
    }
}
```

---

## Task 5: Markdown Importer

### File: `crates/cortex-server/src/import/markdown.rs`

A single markdown file becomes a single node. Front matter (YAML between `---` delimiters) is parsed for metadata.

**Expected format:**
```markdown
---
kind: decision
title: Use FastAPI for the backend
tags: [backend, python]
importance: 0.8
---

We chose FastAPI over Flask for the following reasons:
- Async support via ASGI
- Built-in type validation with Pydantic
- Auto-generated OpenAPI docs
```

If no front matter, the first line (stripped of `#`) is used as the title and the rest as the body. Kind defaults to `"fact"`.

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, NodeKind, Source, Result, CortexError};
use std::path::Path;

pub struct MarkdownImporter;

impl ImportAdapter for MarkdownImporter {
    fn import(&self, path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| CortexError::Validation(format!("Cannot read {}: {}", path.display(), e)))?;

        let (front_matter, body) = parse_front_matter(&content);

        let kind_str = options.kind_override.clone()
            .or_else(|| front_matter.get("kind").cloned())
            .unwrap_or_else(|| "fact".into());
        let kind = NodeKind::new(&kind_str)?;

        let title = front_matter.get("title").cloned()
            .or_else(|| extract_first_heading(&body))
            .unwrap_or_else(|| path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string());

        let importance: f32 = front_matter.get("importance")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5);

        let tags: Vec<String> = front_matter.get("tags")
            .map(|t| t.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect())
            .unwrap_or_default();

        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("import".into()),
        };

        let mut node = Node::new(kind, title, body.trim().to_string(), source, importance);
        node.data.tags = tags;

        Ok(ImportBatch { nodes: vec![node], edges: vec![] })
    }
}

/// Parse YAML front matter delimited by `---`.
fn parse_front_matter(content: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut fm = std::collections::HashMap::new();
    if !content.starts_with("---") {
        return (fm, content.to_string());
    }
    let rest = &content[3..];
    if let Some(end) = rest.find("\n---") {
        let yaml_part = &rest[..end];
        let body = &rest[end + 4..];
        for line in yaml_part.lines() {
            if let Some((k, v)) = line.split_once(':') {
                fm.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        return (fm, body.to_string());
    }
    (fm, content.to_string())
}

fn extract_first_heading(content: &str) -> Option<String> {
    for line in content.lines() {
        let stripped = line.trim_start_matches('#').trim();
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    None
}
```

---

## Task 6: Obsidian Vault Importer

### File: `crates/cortex-server/src/import/obsidian.rs`

This is the **killer feature** for early adoption. Obsidian users have structured vaults with:
- Markdown files (one per note)
- Wikilinks (`[[Note Title]]`) that reference other notes
- Optional YAML front matter with tags and metadata

The importer:
1. Walks the vault directory, creating one node per `.md` file
2. Parses wikilinks and creates edges (`related_to` by default, or a configurable relation)
3. Resolves wikilinks to node IDs by matching note titles
4. Imports tags from front matter

**Cargo.toml dependency:**
```toml
walkdir = "2"
regex = "1"
```

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, Edge, NodeKind, Relation, Source, EdgeProvenance, Result, CortexError};
use std::path::Path;
use std::collections::HashMap;
use walkdir::WalkDir;
use regex::Regex;

pub struct ObsidianImporter;

impl ImportAdapter for ObsidianImporter {
    fn import(&self, vault_path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let wikilink_re = Regex::new(r"\[\[([^\]|]+)(\|[^\]]+)?\]\]").unwrap();

        // Phase 1: Walk vault, parse all notes
        let mut notes: Vec<(String, Node)> = Vec::new(); // (title, node)
        let mut title_to_id: HashMap<String, uuid::Uuid> = HashMap::new();

        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("obsidian-import".into()),
        };

        for entry in WalkDir::new(vault_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
        {
            // Skip .obsidian system directory
            if entry.path().components().any(|c| c.as_os_str() == ".obsidian") {
                continue;
            }

            let content = std::fs::read_to_string(entry.path())
                .map_err(|e| CortexError::Validation(format!("Cannot read {}: {}", entry.path().display(), e)))?;

            let (front_matter, body) = super::markdown::parse_front_matter_pub(&content);

            let title = front_matter.get("title").cloned()
                .or_else(|| super::markdown::extract_first_heading_pub(&body))
                .unwrap_or_else(|| {
                    entry.path().file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

            let kind_str = options.kind_override.clone()
                .or_else(|| front_matter.get("kind").cloned())
                .unwrap_or_else(|| "fact".into());
            let kind = NodeKind::new(&kind_str)?;

            let importance: f32 = front_matter.get("importance")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5);

            let tags: Vec<String> = front_matter.get("tags")
                .map(|t| t.trim_matches(|c: char| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect())
                .unwrap_or_default();

            let mut node = Node::new(kind, title.clone(), body.trim().to_string(), source.clone(), importance);
            node.data.tags = tags;

            title_to_id.insert(title.clone(), node.id);
            notes.push((content.clone(), node));
        }

        // Phase 2: Parse wikilinks and create edges
        let mut edges: Vec<Edge> = Vec::new();
        let nodes: Vec<Node> = notes.iter().map(|(_, n)| n.clone()).collect();

        for (content, node) in &notes {
            for cap in wikilink_re.captures_iter(content) {
                let linked_title = cap.get(1).unwrap().as_str().trim().to_string();
                if let Some(&target_id) = title_to_id.get(&linked_title) {
                    if target_id != node.id {
                        edges.push(Edge::new(
                            node.id,
                            target_id,
                            Relation::new("related_to").unwrap(),
                            0.8,
                            EdgeProvenance::Imported { source: "obsidian-wikilink".into() },
                        ));
                    }
                }
                // Unresolved wikilinks are silently skipped
            }
        }

        tracing::info!(
            "Obsidian import: {} notes parsed, {} wikilink edges created",
            nodes.len(), edges.len()
        );

        Ok(ImportBatch { nodes, edges })
    }
}
```

**Example usage:**
```
$ cortex import --format obsidian ~/vault/

Scanning vault at /Users/mike/vault...
Found 847 notes.
Parsed 847 nodes, 2,341 wikilink edges.
Importing...
✅ Imported 847 nodes, 2,341 edges in 14.2s
```

---

## Task 7: Notion Importer

### File: `crates/cortex-server/src/import/notion.rs`

Notion's export format produces a directory of HTML files (or Markdown, depending on export settings). Each page becomes a node.

**Notion export directory structure (HTML export):**
```
notion-export/
  My Notes/
    Page Title.html
    Subpage.html
    ...
```

**Notion export directory structure (Markdown export):**
```
notion-export/
  My Notes/
    Page Title.md
    Subpage.md
    ...
```

```rust
use super::{ImportAdapter, ImportBatch, ImportOptions};
use cortex_core::{Node, NodeKind, Source, Result, CortexError};
use std::path::Path;
use walkdir::WalkDir;

pub struct NotionImporter;

impl ImportAdapter for NotionImporter {
    fn import(&self, export_path: &Path, options: &ImportOptions) -> Result<ImportBatch> {
        let source = Source {
            agent: options.source_agent.clone(),
            session: None,
            channel: Some("notion-import".into()),
        };

        let mut batch = ImportBatch::default();

        for entry in WalkDir::new(export_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path().extension().and_then(|s| s.to_str());
                matches!(ext, Some("md") | Some("html"))
            })
        {
            let content = std::fs::read_to_string(entry.path())
                .map_err(|e| CortexError::Validation(format!("Cannot read {}: {}", entry.path().display(), e)))?;

            let (title, body) = match entry.path().extension().and_then(|s| s.to_str()) {
                Some("html") => extract_html_content(&content),
                Some("md")   => extract_markdown_content(&content),
                _            => continue,
            };

            let kind_str = options.kind_override.clone().unwrap_or_else(|| "fact".into());
            let kind = NodeKind::new(&kind_str)?;

            let node = Node::new(kind, title, body, source.clone(), 0.5);
            batch.nodes.push(node);
        }

        Ok(batch)
    }
}

/// Strip HTML tags and extract title from <title> or <h1>.
fn extract_html_content(html: &str) -> (String, String) {
    // Simple extraction: strip tags, use first H1 as title
    let title = extract_between(html, "<title>", "</title>")
        .or_else(|| extract_between(html, "<h1>", "</h1>"))
        .unwrap_or("Untitled".into());

    // Strip all HTML tags for body
    let body = strip_html_tags(html);

    (title, body)
}

fn extract_markdown_content(md: &str) -> (String, String) {
    // Same as markdown importer's logic
    let title = md.lines()
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or("Untitled".into());
    (title, md.to_string())
}

fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = s.find(start)? + start.len();
    let end_idx = s[start_idx..].find(end)?;
    Some(s[start_idx..start_idx + end_idx].trim().to_string())
}

fn strip_html_tags(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(s, "").trim().to_string()
}
```

---

## Task 8: File Watcher Upgrade (inotify/FSEvents)

### File: `crates/cortex-server/src/ingest/file_watcher.rs`

Upgrade the existing file watcher to use platform-native APIs for lower latency and proper directory watching.

**Cargo.toml dependency:**
```toml
notify = "6"  # Cross-platform file system events (inotify on Linux, kqueue/FSEvents on macOS, ReadDirectoryChangesW on Windows)
```

```rust
use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use notify::{Watcher, RecursiveMode, Event, EventKind, recommended_watcher};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct FileWatcherAdapter {
    pub watch_dir: PathBuf,
}

#[async_trait]
impl IngestAdapter for FileWatcherAdapter {
    fn name(&self) -> &str { "file-watcher" }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        let (tx, rx) = mpsc::channel::<IngestEvent>(256);
        let watch_dir = self.watch_dir.clone();

        // Spawn watcher on a blocking thread
        tokio::task::spawn_blocking(move || {
            let (notify_tx, notify_rx) = std::sync::mpsc::channel();
            let mut watcher = recommended_watcher(notify_tx).unwrap();
            watcher.watch(&watch_dir, RecursiveMode::Recursive).unwrap();

            for event in notify_rx {
                match event {
                    Ok(Event { kind: EventKind::Create(_) | EventKind::Modify(_), paths, .. }) => {
                        for path in paths {
                            if let Some(event) = file_to_ingest_event(&path) {
                                let _ = tx.blocking_send(event);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }
}

/// Convert a file to an IngestEvent by reading its content.
fn file_to_ingest_event(path: &std::path::Path) -> Option<IngestEvent> {
    // Only process supported extensions
    match path.extension()?.to_str()? {
        "txt" | "md" | "json" => {}
        _ => return None,
    }

    let content = std::fs::read_to_string(path).ok()?;
    let title = path.file_stem()?.to_str()?.to_string();

    Some(IngestEvent {
        kind: "fact".into(),
        title,
        body: content,
        metadata: std::collections::HashMap::new(),
        tags: vec![],
        source: "file-watcher".into(),
        session: None,
        importance: None,
    })
}
```

**Configuration** (already defined in Phase 7A `cortex.toml`):
```toml
[ingest.file]
watch_dir = "./data/ingest"
```

The file watcher is started by `cortex serve` when `ingest.file.watch_dir` is configured.

---

## Definition of Done

- [ ] `cortex import data.json` imports a valid JSON array of nodes
- [ ] `cortex import --format json data.json` is equivalent
- [ ] JSON import with missing `kind` falls back to `"fact"`
- [ ] `cortex import facts.csv` imports a CSV file with kind/title/body/tags/importance columns
- [ ] CSV import handles quoted fields with embedded commas
- [ ] `cortex import stream.jsonl` imports JSONL, one node per line
- [ ] JSONL import skips blank lines without error
- [ ] `cortex import notes.md` creates a single node from a markdown file
- [ ] Markdown import parses YAML front matter for `kind`, `title`, `tags`, `importance`
- [ ] Markdown import falls back to first heading as title when no front matter
- [ ] `cortex import --format obsidian ~/vault/` walks the vault and imports all `.md` files
- [ ] Obsidian import skips `.obsidian/` system directory
- [ ] Obsidian import creates edges for every resolved `[[wikilink]]`
- [ ] Obsidian import silently ignores unresolved wikilinks
- [ ] `cortex import --format notion <export-dir>` imports Notion HTML and MD exports
- [ ] Notion HTML import strips HTML tags correctly
- [ ] `cortex import --dry-run` prints counts but writes nothing to the database
- [ ] `cortex import --source my-agent` assigns `my-agent` as the node source
- [ ] `cortex import --kind document data.json` overrides all node kinds to `document`
- [ ] File watcher uses `notify` crate (inotify/FSEvents/kqueue) instead of polling
- [ ] File watcher creates nodes for new `.md`, `.txt`, `.json` files in `watch_dir`
- [ ] File watcher handles rapid create+modify events without duplicates
- [ ] `cargo test -p cortex-server --test import` passes for all format tests
- [ ] Import of a 10,000-node JSONL file completes in under 60 seconds on a laptop
