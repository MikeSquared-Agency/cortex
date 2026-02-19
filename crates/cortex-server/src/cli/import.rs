use crate::cli::ImportArgs;
use crate::config::CortexConfig;
use anyhow::{Context, Result};
use cortex_core::*;
use std::sync::Arc;

pub async fn run(args: ImportArgs, config: CortexConfig) -> Result<()> {
    let path = &args.file;

    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    // Determine format
    let format = args.format.clone().unwrap_or_else(|| {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => "json",
            Some("jsonl") | Some("ndjson") => "jsonl",
            Some("csv") => "csv",
            Some("md") | Some("markdown") => "markdown",
            _ => "json",
        }
        .to_string()
    });

    println!("Importing {} as {} format...", path.display(), format);

    // Parse nodes from file
    let nodes = match format.as_str() {
        "json" => import_json(path, &args.source)?,
        "jsonl" => import_jsonl(path, &args.source)?,
        "csv" => import_csv(path, &args.source)?,
        "markdown" => import_markdown(path, &args.source)?,
        other => anyhow::bail!("Unknown format: {}", other),
    };

    println!("Parsed {} nodes", nodes.len());

    if args.dry_run {
        println!("Dry run — no changes written.");
        for node in &nodes {
            println!("  [{}] {} ({})", node.kind, node.data.title, node.id);
        }
        return Ok(());
    }

    // Open DB and write nodes with embeddings
    let storage = Arc::new(RedbStorage::open(config.db_path())?);
    let embedding_service = Arc::new(FastEmbedService::new()?);
    let vector_index = Arc::new(std::sync::RwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));

    let mut imported = 0;
    let mut errors = 0;

    for mut node in nodes {
        // Generate embedding
        let text = embedding_input(&node);
        match embedding_service.embed(&text) {
            Ok(emb) => {
                node.embedding = Some(emb.clone());
                if let Err(e) = storage.put_node(&node) {
                    eprintln!("  Error storing node '{}': {}", node.data.title, e);
                    errors += 1;
                    continue;
                }
                if let Ok(mut idx) = vector_index.write() {
                    let _ = idx.insert(node.id, &emb);
                }
                imported += 1;
            }
            Err(e) => {
                eprintln!("  Embedding failed for '{}': {}", node.data.title, e);
                // Store without embedding
                if let Err(e2) = storage.put_node(&node) {
                    eprintln!("  Error storing node: {}", e2);
                    errors += 1;
                } else {
                    imported += 1;
                }
            }
        }
    }

    println!("✅ Imported {} nodes ({} errors)", imported, errors);

    Ok(())
}

fn import_json(path: &std::path::Path, source: &str) -> Result<Vec<Node>> {
    let content = std::fs::read_to_string(path)?;
    let records: Vec<serde_json::Value> =
        serde_json::from_str(&content).context("Failed to parse JSON array")?;
    records.iter().map(|v| json_to_node(v, source)).collect()
}

fn import_jsonl(path: &std::path::Path, source: &str) -> Result<Vec<Node>> {
    let content = std::fs::read_to_string(path)?;
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let v: serde_json::Value =
                serde_json::from_str(line).context("Failed to parse JSONL line")?;
            json_to_node(&v, source)
        })
        .collect()
}

fn json_to_node(v: &serde_json::Value, source: &str) -> Result<Node> {
    let kind_str = v["kind"].as_str().unwrap_or("fact");
    let kind = NodeKind::new(kind_str)
        .map_err(|e| anyhow::anyhow!("Invalid kind '{}': {}", kind_str, e))?;

    let title = v["title"].as_str().unwrap_or("Untitled").to_string();
    let body = v["body"].as_str().unwrap_or(title.as_str()).to_string();
    let importance = v["importance"].as_f64().unwrap_or(0.5) as f32;

    let tags: Vec<String> = v["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let agent = v["source_agent"].as_str().unwrap_or(source).to_string();

    let mut node = Node::new(
        kind,
        title,
        body,
        Source {
            agent,
            session: None,
            channel: None,
        },
        importance,
    );
    node.data.tags = tags;

    Ok(node)
}

fn import_csv(path: &std::path::Path, source: &str) -> Result<Vec<Node>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut nodes = Vec::new();

    for result in rdr.records() {
        let record = result?;

        let kind_str = record.get(0).unwrap_or("fact");
        let title = record.get(1).unwrap_or("Untitled").to_string();
        let body = record.get(2).unwrap_or(title.as_str()).to_string();
        let tags_str = record.get(3).unwrap_or("");

        let kind = NodeKind::new(kind_str)
            .map_err(|e| anyhow::anyhow!("Invalid kind '{}': {}", kind_str, e))?;

        let tags: Vec<String> = tags_str
            .split(';')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();

        let mut node = Node::new(
            kind,
            title,
            body,
            Source {
                agent: source.to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );
        node.data.tags = tags;
        nodes.push(node);
    }

    Ok(nodes)
}

fn import_markdown(path: &std::path::Path, source: &str) -> Result<Vec<Node>> {
    let content = std::fs::read_to_string(path)?;
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    let node = Node::new(
        NodeKind::new("fact").unwrap(),
        title,
        content,
        Source {
            agent: source.to_string(),
            session: None,
            channel: None,
        },
        0.5,
    );

    Ok(vec![node])
}
