//! MCP (Model Context Protocol) server — stdio JSON-RPC transport.
//!
//! Exposes Cortex graph memory as tools and resources to any MCP-compatible AI agent
//! (Claude Desktop, Cursor, Cline, etc.) without requiring `cortex serve`.
//!
//! Protocol: JSON-RPC 2.0 over stdin/stdout. All logs go to stderr.

use anyhow::Result;
use cortex_core::{
    Cortex, Edge, EdgeProvenance, LibraryConfig, Node, NodeFilter, NodeId, NodeKind, Relation,
    Source,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

pub struct McpArgs {
    pub data_dir: Option<PathBuf>,
    pub server: Option<String>,
}

pub async fn run(args: McpArgs) -> Result<()> {
    if let Some(ref server) = args.server {
        return run_remote(server).await;
    }

    let data_dir = args
        .data_dir
        .map(expand_home)
        .unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("cortex.redb");
    eprintln!("[cortex-mcp] Opening database: {}", db_path.display());
    eprintln!("[cortex-mcp] Initializing embedding model (first run may download model files)...");

    let cortex = Cortex::open(&db_path, LibraryConfig::default())?;
    eprintln!("[cortex-mcp] Ready. Listening on stdio (JSON-RPC 2.0).");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut out = tokio::io::BufWriter::new(stdout);

    while let Some(line) = reader.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if let Some(response) = dispatch(&cortex, &line) {
            let bytes = serde_json::to_vec(&response)?;
            out.write_all(&bytes).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }

    eprintln!("[cortex-mcp] Stdin closed. Shutting down.");
    Ok(())
}

fn expand_home(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path
}

fn default_data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".cortex").join("default"))
        .unwrap_or_else(|_| PathBuf::from(".cortex"))
}

/// Parse an incoming JSON-RPC message and produce a response (if any).
/// Notifications (no `id`) return None.
fn dispatch(cortex: &Cortex, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[cortex-mcp] Parse error: {e}");
            return Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {e}")}
            }));
        }
    };

    // Notifications have no "id" field — must not respond
    let id = match msg.get("id") {
        Some(id) => id.clone(),
        None => return None,
    };

    let method = msg["method"].as_str().unwrap_or("").to_string();
    let params = msg
        .get("params")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    let result = route(cortex, &method, &params);

    Some(match result {
        Ok(value) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": value,
        }),
        Err(e) => {
            eprintln!("[cortex-mcp] Error in {method}: {e}");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32603, "message": e.to_string()},
            })
        }
    })
}

fn route(cortex: &Cortex, method: &str, params: &Value) -> Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
                "resources": {}
            },
            "serverInfo": {
                "name": "cortex",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Cortex is a persistent graph memory engine for AI agents. \
                Use cortex_store to remember facts, decisions, goals, and observations. \
                Use cortex_observe after interactions to record performance metrics. \
                Use cortex_search or cortex_recall to retrieve relevant knowledge. \
                Use cortex_briefing at session start for a structured overview. \
                Use cortex_traverse to explore how concepts connect. \
                Use cortex_relate to explicitly link related nodes."
        })),

        "tools/list" => Ok(tools_schema()),

        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            let text = call_tool(cortex, name, &args)?;
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "isError": false,
            }))
        }

        "resources/list" => Ok(json!({
            "resources": [
                {
                    "uri": "cortex://stats",
                    "name": "Graph Statistics",
                    "description": "Current graph memory statistics: node count, edge count, per-kind breakdown, oldest/newest node.",
                    "mimeType": "application/json"
                },
                {
                    "uri": "cortex://node/{id}",
                    "name": "Knowledge Node",
                    "description": "A single node from graph memory with metadata, edges, and related nodes. Replace {id} with a node UUID.",
                    "mimeType": "application/json"
                }
            ]
        })),

        "resources/read" => {
            let uri = params["uri"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("uri required"))?;
            read_resource(cortex, uri)
        }

        "ping" => Ok(json!({})),

        _ => Err(anyhow::anyhow!("Method not found: {}", method)),
    }
}

// ── Tool schemas ─────────────────────────────────────────────────────────────

fn tools_schema() -> Value {
    json!({
        "tools": [
            {
                "name": "cortex_store",
                "description": "Store a piece of knowledge in persistent graph memory. Use this to remember facts, decisions, goals, events, patterns, and observations across sessions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "description": "Node type: fact, decision, goal, event, pattern, observation, preference",
                            "default": "fact"
                        },
                        "title": {
                            "type": "string",
                            "description": "Short summary (used for search and dedup)"
                        },
                        "body": {
                            "type": "string",
                            "description": "Full content. Can be long."
                        },
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional tags for filtering. Lowercase alphanumeric + hyphens only."
                        },
                        "importance": {
                            "type": "number",
                            "description": "0.0 to 1.0. Higher = retained longer, weighted more in search.",
                            "default": 0.5
                        }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "cortex_search",
                "description": "Search graph memory by meaning. Returns the most relevant nodes ranked by semantic similarity.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Natural language search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max results to return",
                            "default": 10
                        },
                        "kind": {
                            "type": "string",
                            "description": "Optional: filter by node kind (e.g. fact, goal, decision)"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "cortex_recall",
                "description": "Recall knowledge using hybrid search (semantic + graph structure). Better than cortex_search when you need contextually related information, not just similar text.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "What to recall"
                        },
                        "limit": {
                            "type": "integer",
                            "default": 10
                        },
                        "alpha": {
                            "type": "number",
                            "description": "Balance: 0.0 = pure graph, 1.0 = pure vector. Default 0.7",
                            "default": 0.7
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "cortex_briefing",
                "description": "Generate a context briefing from graph memory. Returns a structured summary of relevant knowledge including active goals, recent decisions, patterns, and key facts. Use at the start of a session or when you need a broad overview.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": {
                            "type": "string",
                            "description": "Agent identifier for personalised briefings",
                            "default": "default"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, returns a shorter ~4x denser briefing",
                            "default": false
                        }
                    }
                }
            },
            {
                "name": "cortex_traverse",
                "description": "Explore connections from a node in the knowledge graph. Reveals how concepts relate to each other.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "node_id": {
                            "type": "string",
                            "description": "Starting node UUID"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "How many hops to explore",
                            "default": 2
                        },
                        "direction": {
                            "type": "string",
                            "enum": ["outgoing", "incoming", "both"],
                            "default": "both"
                        }
                    },
                    "required": ["node_id"]
                }
            },
            {
                "name": "cortex_relate",
                "description": "Create a relationship between two nodes in the knowledge graph. Use to explicitly connect related concepts.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "from_id": {
                            "type": "string",
                            "description": "Source node UUID"
                        },
                        "to_id": {
                            "type": "string",
                            "description": "Target node UUID"
                        },
                        "relation": {
                            "type": "string",
                            "description": "Relationship type: relates_to, supports, contradicts, caused_by, depends_on, similar_to, supersedes",
                            "default": "relates_to"
                        }
                    },
                    "required": ["from_id", "to_id"]
                }
            },
            {
                "name": "cortex_observe",
                "description": "Record a performance observation for an agent's prompt variant. Call this after interactions to track how well the current prompt is performing. Feeds into automatic variant selection and rollback.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_name": {
                            "type": "string",
                            "description": "Name of the agent (e.g. 'kai')"
                        },
                        "variant_slug": {
                            "type": "string",
                            "description": "Slug/title of the active prompt variant"
                        },
                        "variant_id": {
                            "type": "string",
                            "description": "UUID of the active prompt variant node"
                        },
                        "sentiment_score": {
                            "type": "number",
                            "description": "User sentiment: 0.0 (frustrated) to 1.0 (pleased). Default: 0.5"
                        },
                        "correction_count": {
                            "type": "integer",
                            "description": "Number of user corrections in this interaction. Default: 0"
                        },
                        "task_outcome": {
                            "type": "string",
                            "description": "Outcome: success, partial, failure, or unknown. Default: unknown",
                            "enum": ["success", "partial", "failure", "unknown"]
                        },
                        "task_type": {
                            "type": "string",
                            "description": "Type of task: coding, planning, casual, crisis, reflection. Default: casual"
                        },
                        "token_cost": {
                            "type": "integer",
                            "description": "Total tokens consumed (optional)"
                        }
                    },
                    "required": ["agent_name", "variant_slug", "variant_id"]
                }
            }
        ]
    })
}

// ── Tool implementations ──────────────────────────────────────────────────────

fn call_tool(cortex: &Cortex, name: &str, args: &Value) -> Result<String> {
    match name {
        "cortex_store" => tool_store(cortex, args),
        "cortex_search" => tool_search(cortex, args),
        "cortex_recall" => tool_recall(cortex, args),
        "cortex_briefing" => tool_briefing(cortex, args),
        "cortex_traverse" => tool_traverse(cortex, args),
        "cortex_relate" => tool_relate(cortex, args),
        "cortex_observe" => tool_observe(cortex, args),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

fn tool_store(cortex: &Cortex, args: &Value) -> Result<String> {
    let kind_str = args["kind"].as_str().unwrap_or("fact");
    let title = args["title"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("title is required"))?
        .to_string();
    let body = args
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or(&title)
        .to_string();
    let importance = args
        .get("importance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;

    // Normalise tags: lowercase, spaces→hyphens, drop invalid chars
    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_lowercase().replace(' ', "-"))
                .filter(|s| {
                    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                })
                .collect()
        })
        .unwrap_or_default();

    let kind = NodeKind::new(kind_str)
        .map_err(|e| anyhow::anyhow!("Invalid kind '{}': {}", kind_str, e))?;

    let mut node = Node::new(
        kind,
        title.clone(),
        body,
        Source {
            agent: "mcp".into(),
            session: None,
            channel: None,
        },
        importance,
    );
    node.data.tags = tags;

    let id = cortex.store(node)?;
    Ok(serde_json::to_string(&json!({
        "id": id.to_string(),
        "message": format!("Stored: {title}"),
    }))?)
}

fn tool_search(cortex: &Cortex, args: &Value) -> Result<String> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let kind_filter = args.get("kind").and_then(|v| v.as_str()).map(String::from);

    // Fetch extra results when kind-filtering so we hit the requested limit
    let fetch = if kind_filter.is_some() {
        (limit * 4).max(1)
    } else {
        limit.max(1)
    };

    let mut results = cortex.search(query, fetch).unwrap_or_default();
    if let Some(ref k) = kind_filter {
        results.retain(|(_, n)| n.kind.as_str() == k.as_str());
    }
    results.truncate(limit);

    let items: Vec<Value> = results
        .iter()
        .map(|(score, n)| {
            json!({
                "id": n.id.to_string(),
                "kind": n.kind.as_str(),
                "title": n.data.title,
                "body": n.data.body,
                "score": score,
                "created_at": n.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&items)?)
}

fn tool_recall(cortex: &Cortex, args: &Value) -> Result<String> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let _alpha = args.get("alpha").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;

    // Phase 1: vector search
    let seeds = cortex.search(query, limit).unwrap_or_default();

    // Phase 2: graph expansion — include 1-hop neighbours of top results
    let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
    let mut expanded: Vec<(f32, Node)> = Vec::new();

    for (score, node) in &seeds {
        if seen.insert(node.id) {
            expanded.push((*score, node.clone()));
        }
        if expanded.len() < limit * 2 {
            if let Ok(sg) = cortex.traverse(node.id, 1) {
                for neighbour in sg.nodes.values() {
                    if seen.insert(neighbour.id) {
                        // Neighbours get a discounted score
                        expanded.push((score * 0.6, neighbour.clone()));
                    }
                }
            }
        }
    }

    expanded.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    expanded.truncate(limit);

    let items: Vec<Value> = expanded
        .iter()
        .map(|(score, n)| {
            json!({
                "id": n.id.to_string(),
                "kind": n.kind.as_str(),
                "title": n.data.title,
                "body": n.data.body,
                "score": score,
                "created_at": n.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&items)?)
}

fn tool_briefing(cortex: &Cortex, args: &Value) -> Result<String> {
    let _agent_id = args
        .get("agent_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let compact = args
        .get("compact")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = if compact { 3 } else { 8 };

    let mut md = String::from("# Context Briefing\n\n");
    let mut has_content = false;

    let sections: &[(&str, &str)] = &[
        ("goal", "## Active Goals"),
        ("decision", "## Recent Decisions"),
        ("pattern", "## Patterns"),
        ("observation", "## Observations"),
    ];

    for (kind_str, heading) in sections {
        let kind = NodeKind::new(kind_str).unwrap();
        let nodes = cortex
            .list_nodes(NodeFilter::new().with_kinds(vec![kind]).with_limit(limit))
            .unwrap_or_default();

        if !nodes.is_empty() {
            has_content = true;
            md.push_str(heading);
            md.push('\n');
            for n in &nodes {
                if compact {
                    md.push_str(&format!("- {}\n", n.data.title));
                } else if !n.data.body.is_empty() && n.data.body != n.data.title {
                    md.push_str(&format!("- **{}**: {}\n", n.data.title, n.data.body));
                } else {
                    md.push_str(&format!("- {}\n", n.data.title));
                }
            }
            md.push('\n');
        }
    }

    // High-importance facts
    let facts = cortex
        .list_nodes(
            NodeFilter::new()
                .with_kinds(vec![NodeKind::new("fact").unwrap()])
                .with_min_importance(0.7)
                .with_limit(limit),
        )
        .unwrap_or_default();

    if !facts.is_empty() {
        has_content = true;
        md.push_str("## Key Facts\n");
        for n in &facts {
            md.push_str(&format!("- {}\n", n.data.title));
        }
        md.push('\n');
    }

    if !has_content {
        md.push_str("*No memory stored yet. Use `cortex_store` to add knowledge.*\n");
    }

    Ok(serde_json::to_string(&json!({"briefing": md}))?)
}

fn tool_traverse(cortex: &Cortex, args: &Value) -> Result<String> {
    let node_id_str = args["node_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("node_id is required"))?;
    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
    let _direction = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("both");

    let node_id: NodeId = Uuid::parse_str(node_id_str)
        .map_err(|_| anyhow::anyhow!("Invalid node_id: not a valid UUID"))?;

    let sg = cortex.traverse(node_id, depth)?;

    let nodes: Vec<Value> = sg
        .nodes
        .values()
        .map(|n| {
            json!({
                "id": n.id.to_string(),
                "kind": n.kind.as_str(),
                "title": n.data.title,
                "body": n.data.body,
                "importance": n.importance,
                "depth": sg.depths.get(&n.id).copied().unwrap_or(0),
            })
        })
        .collect();

    let edges: Vec<Value> = sg
        .edges
        .iter()
        .map(|e| {
            json!({
                "id": e.id.to_string(),
                "from": e.from.to_string(),
                "to": e.to.to_string(),
                "relation": e.relation.as_str(),
                "weight": e.weight,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json!({
        "nodes": nodes,
        "edges": edges,
        "node_count": sg.nodes.len(),
        "edge_count": sg.edges.len(),
        "truncated": sg.truncated,
    }))?)
}

fn tool_relate(cortex: &Cortex, args: &Value) -> Result<String> {
    let from_str = args["from_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("from_id is required"))?;
    let to_str = args["to_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("to_id is required"))?;

    // Accept both hyphenated (relates-to) and underscored (relates_to) forms
    let relation_raw = args
        .get("relation")
        .and_then(|v| v.as_str())
        .unwrap_or("relates_to");
    let relation_str = relation_raw.replace('-', "_");

    let from_id: NodeId =
        Uuid::parse_str(from_str).map_err(|_| anyhow::anyhow!("Invalid from_id: not a UUID"))?;
    let to_id: NodeId =
        Uuid::parse_str(to_str).map_err(|_| anyhow::anyhow!("Invalid to_id: not a UUID"))?;

    let from_title = cortex
        .get_node(from_id)?
        .map(|n| n.data.title)
        .unwrap_or_else(|| from_str.to_string());
    let to_title = cortex
        .get_node(to_id)?
        .map(|n| n.data.title)
        .unwrap_or_else(|| to_str.to_string());

    let relation = Relation::new(&relation_str)
        .map_err(|e| anyhow::anyhow!("Invalid relation '{}': {}", relation_str, e))?;

    let edge = Edge::new(
        from_id,
        to_id,
        relation.clone(),
        1.0,
        EdgeProvenance::Manual {
            created_by: "mcp".into(),
        },
    );
    let edge_id = edge.id;
    cortex.create_edge(edge)?;

    Ok(serde_json::to_string(&json!({
        "id": edge_id.to_string(),
        "message": format!("Related: {} → {} → {}", from_title, relation_str, to_title),
    }))?)
}


fn tool_observe(cortex: &Cortex, args: &Value) -> Result<String> {
    let agent_name = args["agent_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("agent_name is required"))?;
    let variant_slug = args["variant_slug"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("variant_slug is required"))?;
    let variant_id_str = args["variant_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("variant_id is required"))?;
    let variant_id: NodeId = Uuid::parse_str(variant_id_str)
        .map_err(|_| anyhow::anyhow!("Invalid variant_id: not a UUID"))?;

    let sentiment_score = args.get("sentiment_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;
    let correction_count = args.get("correction_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let task_outcome = args.get("task_outcome")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let task_type = args.get("task_type")
        .and_then(|v| v.as_str())
        .unwrap_or("casual");
    let token_cost = args.get("token_cost")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    use cortex_core::prompt::selection as sel;

    // Compute observation score
    let obs_score = sel::observation_score(sentiment_score, correction_count, task_outcome);

    // Create observation node
    let obs_body = format!(
        "Sentiment: {:.2}, Corrections: {}, Outcome: {}, Task: {}",
        sentiment_score, correction_count, task_outcome, task_type
    );
    let mut obs_node = Node::new(
        cortex_core::kinds::defaults::observation(),
        format!("{}: Performance for {}", task_outcome, variant_slug),
        obs_body,
        Source { agent: agent_name.to_string(), session: None, channel: None },
        obs_score,
    );
    obs_node.data.metadata.insert("observation_type".into(), json!("performance"));
    obs_node.data.metadata.insert("variant_id".into(), json!(variant_id_str));
    obs_node.data.metadata.insert("variant_slug".into(), json!(variant_slug));
    obs_node.data.metadata.insert("sentiment_score".into(), json!(sentiment_score));
    obs_node.data.metadata.insert("correction_count".into(), json!(correction_count));
    obs_node.data.metadata.insert("task_outcome".into(), json!(task_outcome));
    obs_node.data.metadata.insert("observation_score".into(), json!(obs_score));
    if let Some(tc) = token_cost {
        obs_node.data.metadata.insert("token_cost".into(), json!(tc));
    }

    let obs_id = obs_node.id;
    cortex.store(obs_node)?;

    // Link: agent --performed--> observation (best-effort — skip if agent node not found)
    let agent_kind = cortex_core::kinds::defaults::agent();
    if let Ok(agents) = cortex.list_nodes(cortex_core::storage::NodeFilter::new().with_kinds(vec![agent_kind])) {
        if let Some(agent) = agents.into_iter().find(|n| n.data.title == agent_name) {
            let _ = cortex.create_edge(Edge::new(
                agent.id,
                obs_id,
                cortex_core::relations::defaults::performed(),
                1.0,
                EdgeProvenance::Manual { created_by: "mcp".into() },
            ));
        }
    }

    // Link: observation --informed_by--> variant
    let _ = cortex.create_edge(Edge::new(
        obs_id,
        variant_id,
        cortex_core::relations::defaults::informed_by(),
        1.0,
        EdgeProvenance::Manual { created_by: "mcp".into() },
    ));

    Ok(serde_json::to_string(&json!({
        "observation_id": obs_id.to_string(),
        "observation_score": obs_score,
        "variant_slug": variant_slug,
        "sentiment_score": sentiment_score,
        "correction_count": correction_count,
        "task_outcome": task_outcome,
        "message": format!("Observation recorded: score={:.3} for variant '{}'", obs_score, variant_slug),
    }))?)
}

// ── Resource handlers ─────────────────────────────────────────────────────────

fn read_resource(cortex: &Cortex, uri: &str) -> Result<Value> {
    if uri == "cortex://stats" {
        return resource_stats(cortex);
    }
    if let Some(id_str) = uri.strip_prefix("cortex://node/") {
        return resource_node(cortex, uri, id_str);
    }
    Err(anyhow::anyhow!("Unknown resource URI: {}", uri))
}

fn resource_stats(cortex: &Cortex) -> Result<Value> {
    let all_nodes = cortex.list_nodes(NodeFilter::new()).unwrap_or_default();
    let node_count = all_nodes.len() as u64;

    let mut by_kind: std::collections::HashMap<String, u64> = Default::default();
    let mut oldest: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut newest: Option<chrono::DateTime<chrono::Utc>> = None;

    for n in &all_nodes {
        *by_kind.entry(n.kind.as_str().to_string()).or_insert(0) += 1;
        oldest = Some(match oldest {
            None => n.created_at,
            Some(t) if n.created_at < t => n.created_at,
            Some(t) => t,
        });
        newest = Some(match newest {
            None => n.created_at,
            Some(t) if n.created_at > t => n.created_at,
            Some(t) => t,
        });
    }

    let stats = json!({
        "node_count": node_count,
        "node_counts_by_kind": by_kind,
        "oldest_node": oldest.map(|t| t.to_rfc3339()),
        "newest_node": newest.map(|t| t.to_rfc3339()),
    });

    Ok(json!({
        "contents": [{
            "uri": "cortex://stats",
            "mimeType": "application/json",
            "text": serde_json::to_string_pretty(&stats)?
        }]
    }))
}

fn resource_node(cortex: &Cortex, uri: &str, id_str: &str) -> Result<Value> {
    let node_id: NodeId = Uuid::parse_str(id_str)
        .map_err(|_| anyhow::anyhow!("Invalid node ID in URI: {}", id_str))?;

    let node = cortex
        .get_node(node_id)?
        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", id_str))?;

    // 1-hop traversal to get edges and neighbours
    let sg = cortex.traverse(node_id, 1).unwrap_or_default();

    let edges: Vec<Value> = sg
        .edges
        .iter()
        .map(|e| {
            json!({
                "id": e.id.to_string(),
                "from": e.from.to_string(),
                "to": e.to.to_string(),
                "relation": e.relation.as_str(),
                "weight": e.weight,
            })
        })
        .collect();

    let related: Vec<Value> = sg
        .nodes
        .values()
        .filter(|n| n.id != node_id)
        .map(|n| {
            json!({
                "id": n.id.to_string(),
                "kind": n.kind.as_str(),
                "title": n.data.title,
            })
        })
        .collect();

    let node_json = json!({
        "id": node.id.to_string(),
        "kind": node.kind.as_str(),
        "title": node.data.title,
        "body": node.data.body,
        "importance": node.importance,
        "tags": node.data.tags,
        "source_agent": node.source.agent,
        "created_at": node.created_at.to_rfc3339(),
        "updated_at": node.updated_at.to_rfc3339(),
        "access_count": node.access_count,
        "edges": edges,
        "related": related,
    });

    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": serde_json::to_string_pretty(&node_json)?
        }]
    }))
}

// ─── Remote mode: MCP ↔ gRPC proxy ───────────────────────────────────────

async fn run_remote(server_addr: &str) -> Result<()> {
    let base_url = server_addr.trim_end_matches('/').to_string();
    eprintln!("[cortex-mcp] Using remote HTTP server: {}", base_url);
    let http = reqwest::Client::new();
    eprintln!("[cortex-mcp] Ready. Listening on stdio (JSON-RPC 2.0).");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut out = tokio::io::BufWriter::new(stdout);

    while let Some(line) = reader.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if let Some(response) = dispatch_remote(&http, &base_url, &line).await {
            let bytes = serde_json::to_vec(&response)?;
            out.write_all(&bytes).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

async fn dispatch_remote(http: &reqwest::Client, base_url: &str, line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method")?.as_str()?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": "cortex", "version": "0.1.0" }
        })),
        "notifications/initialized" => return None,
        "tools/list" => {
            // Reuse the same tool list
            Ok(tools_list())
        }
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            remote_tool_call(http, base_url, name, &args).await
        }
        "resources/list" => Ok(resources_list()),
        "resources/read" => {
            let uri = req
                .get("params")
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .unwrap_or("");
            remote_resource_read(http, base_url, uri).await
        }
        "ping" => Ok(json!({})),
        _ => Err(anyhow::anyhow!("Unknown method: {}", method)),
    };

    Some(match result {
        Ok(val) => json!({ "jsonrpc": "2.0", "id": id, "result": val }),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32000, "message": e.to_string() }
        }),
    })
}

/// Extract the tools list JSON (reusable between local and remote)
fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "cortex_store",
                "description": "Store a piece of knowledge in persistent graph memory",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "default": "fact" },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "importance": { "type": "number", "default": 0.5 }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "cortex_search",
                "description": "Search graph memory by meaning",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "default": 10 },
                        "kind": { "type": "string" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "cortex_recall",
                "description": "Hybrid search combining semantic similarity and graph structure",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "default": 10 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "cortex_briefing",
                "description": "Generate a context briefing from graph memory",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string", "default": "default" },
                        "compact": { "type": "boolean", "default": false }
                    }
                }
            },
            {
                "name": "cortex_traverse",
                "description": "Explore connections from a node in the knowledge graph",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "node_id": { "type": "string" },
                        "depth": { "type": "integer", "default": 2 },
                        "direction": { "type": "string", "default": "both" }
                    },
                    "required": ["node_id"]
                }
            },
            {
                "name": "cortex_relate",
                "description": "Create a relationship between two nodes",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "from_id": { "type": "string" },
                        "to_id": { "type": "string" },
                        "relation": { "type": "string", "default": "relates-to" }
                    },
                    "required": ["from_id", "to_id"]
                }
            },
            {
                "name": "cortex_observe",
                "description": "Record a performance observation for an agent's prompt variant",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "agent_name": { "type": "string" },
                        "variant_slug": { "type": "string" },
                        "variant_id": { "type": "string" },
                        "sentiment_score": { "type": "number", "default": 0.5 },
                        "correction_count": { "type": "integer", "default": 0 },
                        "task_outcome": { "type": "string", "default": "unknown" },
                        "task_type": { "type": "string", "default": "casual" },
                        "token_cost": { "type": "integer" }
                    },
                    "required": ["agent_name", "variant_slug", "variant_id"]
                }
            }
        ]
    })
}

fn resources_list() -> Value {
    json!({
        "resources": [
            { "uri": "cortex://stats", "name": "Graph Statistics", "mimeType": "application/json" },
            { "uri": "cortex://node/{id}", "name": "Knowledge Node", "mimeType": "application/json" }
        ]
    })
}

async fn remote_tool_call(
    http: &reqwest::Client,
    base_url: &str,
    name: &str,
    args: &Value,
) -> Result<Value> {
    match name {
        "cortex_store" => {
            let resp: Value = http
                .post(format!("{}/nodes", base_url))
                .json(&json!({
                    "kind": args.get("kind").and_then(|v| v.as_str()).unwrap_or("fact"),
                    "title": args.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    "body": args.get("body").and_then(|v| v.as_str()),
                    "tags": args.get("tags"),
                    "importance": args.get("importance"),
                    "source_agent": "mcp",
                }))
                .send()
                .await?
                .json()
                .await?;
            let data = &resp["data"];
            let title = data["title"].as_str().unwrap_or("");
            let id = data["id"].as_str().unwrap_or("");
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Stored: {} (id: {})", title, id) }]
            }))
        }
        "cortex_search" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
            let resp: Value = http
                .get(format!(
                    "{}/search?q={}&limit={}",
                    base_url,
                    urlencoding::encode(query),
                    limit
                ))
                .send()
                .await?
                .json()
                .await?;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp["data"])? }]
            }))
        }
        "cortex_recall" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
            let resp: Value = http
                .get(format!(
                    "{}/search/hybrid?q={}&limit={}",
                    base_url,
                    urlencoding::encode(query),
                    limit
                ))
                .send()
                .await?
                .json()
                .await?;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp["data"])? }]
            }))
        }
        "cortex_briefing" => {
            let agent_id = args
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let compact = args
                .get("compact")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let resp: Value = http
                .get(format!(
                    "{}/briefing/{}?compact={}",
                    base_url,
                    urlencoding::encode(agent_id),
                    compact
                ))
                .send()
                .await?
                .json()
                .await?;
            let rendered = resp["data"]["rendered"]
                .as_str()
                .unwrap_or("No briefing available");
            Ok(json!({
                "content": [{ "type": "text", "text": rendered }]
            }))
        }
        "cortex_traverse" => {
            let node_id = args.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2);
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("both");
            let resp: Value = http
                .get(format!(
                    "{}/nodes/{}/neighbors?depth={}&direction={}",
                    base_url, node_id, depth, direction
                ))
                .send()
                .await?
                .json()
                .await?;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp["data"])? }]
            }))
        }
        "cortex_observe" => {
            let agent_name = args.get("agent_name").and_then(|v| v.as_str()).unwrap_or("");
            let resp: Value = http
                .post(format!("{}/agents/{}/observe", base_url, agent_name))
                .json(&args)
                .send()
                .await?
                .json()
                .await?;
            let obs_id = resp["data"]["observation_id"].as_str().unwrap_or("unknown");
            let score = resp["data"]["observation_score"].as_f64().unwrap_or(0.0);
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Observation recorded: id={}, score={:.3}", obs_id, score) }]
            }))
        }
        "cortex_relate" => {
            let from_id = args.get("from_id").and_then(|v| v.as_str()).unwrap_or("");
            let to_id = args.get("to_id").and_then(|v| v.as_str()).unwrap_or("");
            let relation = args
                .get("relation")
                .and_then(|v| v.as_str())
                .unwrap_or("relates-to");
            let resp: Value = http
                .post(format!("{}/edges", base_url))
                .json(&json!({ "from_id": from_id, "to_id": to_id, "relation": relation }))
                .send()
                .await?
                .json()
                .await?;
            let id = resp["data"]["id"].as_str().unwrap_or("");
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Related: {} -> [{}] -> {} (edge: {})", from_id, relation, to_id, id) }]
            }))
        }
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

async fn remote_resource_read(http: &reqwest::Client, base_url: &str, uri: &str) -> Result<Value> {
    if uri == "cortex://stats" {
        let resp: Value = http
            .get(format!("{}/stats", base_url))
            .send()
            .await?
            .json()
            .await?;
        Ok(json!({
            "contents": [{ "uri": uri, "mimeType": "application/json", "text": serde_json::to_string_pretty(&resp["data"])? }]
        }))
    } else if let Some(id) = uri.strip_prefix("cortex://node/") {
        let resp: Value = http
            .get(format!("{}/nodes/{}", base_url, id))
            .send()
            .await?
            .json()
            .await?;
        Ok(json!({
            "contents": [{ "uri": uri, "mimeType": "application/json", "text": serde_json::to_string_pretty(&resp["data"])? }]
        }))
    } else {
        Err(anyhow::anyhow!("Unknown resource: {}", uri))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cortex() -> Cortex {
        let dir = tempfile::tempdir().unwrap();
        Cortex::open(dir.path().join("test.redb"), LibraryConfig::default()).unwrap()
    }

    #[test]
    fn test_dispatch_initialize() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        assert_eq!(resp["id"], 1);
        assert!(resp["result"]["protocolVersion"].as_str().is_some());
        assert!(resp["result"]["capabilities"].is_object());
    }

    #[test]
    fn test_dispatch_tools_list() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"cortex_store"));
        assert!(names.contains(&"cortex_search"));
        assert!(names.contains(&"cortex_recall"));
        assert!(names.contains(&"cortex_briefing"));
        assert!(names.contains(&"cortex_traverse"));
        assert!(names.contains(&"cortex_relate"));
        assert!(names.contains(&"cortex_observe"));
        assert_eq!(tools.len(), 7);
    }

    #[test]
    fn test_dispatch_resources_list() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":3,"method":"resources/list","params":{}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        let resources = resp["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 2);
        let uris: Vec<&str> = resources
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        assert!(uris.contains(&"cortex://stats"));
        assert!(uris.contains(&"cortex://node/{id}"));
    }

    #[test]
    fn test_notification_no_response() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let resp = dispatch(&cortex, msg);
        assert!(resp.is_none(), "Notifications must not produce a response");
    }

    #[test]
    fn test_parse_error_returns_error_response() {
        let cortex = make_cortex();
        let resp = dispatch(&cortex, "this is not json").unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":99,"method":"nonexistent","params":{}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        assert!(resp["error"].is_object());
    }

    #[test]
    fn test_tools_store_missing_title() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"cortex_store","arguments":{"kind":"fact"}}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        // Missing title should produce an error
        assert!(resp.get("error").is_some() || resp["result"]["isError"] == true);
    }

    #[test]
    fn test_resource_stats_empty_graph() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":20,"method":"resources/read","params":{"uri":"cortex://stats"}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        assert!(resp["result"]["contents"].is_array());
        let text = resp["result"]["contents"][0]["text"].as_str().unwrap();
        let stats: Value = serde_json::from_str(text).unwrap();
        assert_eq!(stats["node_count"], 0);
    }

    #[test]
    fn test_briefing_empty_graph() {
        let cortex = make_cortex();
        let msg = r#"{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"cortex_briefing","arguments":{}}}"#;
        let resp = dispatch(&cortex, msg).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let val: Value = serde_json::from_str(text).unwrap();
        assert!(val["briefing"].as_str().unwrap().contains("No memory"));
    }
}
