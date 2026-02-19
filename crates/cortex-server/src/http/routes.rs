use super::{AppResult, AppState, JsonResponse, GRAPH_VIZ_HTML};
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use cortex_core::{NodeFilter, NodeKind, *};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/nodes", get(list_nodes))
        .route("/nodes/:id", get(get_node))
        .route("/nodes/:id/neighbors", get(node_neighbors))
        .route("/edges/:id", get(get_edge))
        .route("/search", get(search))
        .route("/graph/viz", get(graph_viz))
        .route("/graph/export", get(graph_export))
        .route("/auto-linker/status", get(auto_linker_status))
        .route("/auto-linker/trigger", post(trigger_auto_link))
        .route("/briefing/:agent_id", get(get_briefing))
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    healthy: bool,
    version: String,
    uptime_seconds: u64,
    stats: StatsData,
}

#[derive(Serialize)]
struct StatsData {
    node_count: u64,
    edge_count: u64,
    nodes_by_kind: HashMap<String, u64>,
    edges_by_relation: HashMap<String, u64>,
    db_size_bytes: u64,
}

async fn health(State(state): State<AppState>) -> AppResult<Json<JsonResponse<HealthResponse>>> {
    let stats = state.storage.stats()?;
    let db_size = std::fs::metadata(state.storage.path())
        .map(|m| m.len())
        .unwrap_or(0);

    let nodes_by_kind = stats
        .node_counts_by_kind
        .into_iter()
        .map(|(k, v)| (format!("{:?}", k), v))
        .collect();

    let edges_by_relation = stats
        .edge_counts_by_relation
        .into_iter()
        .map(|(r, v)| (format!("{:?}", r), v))
        .collect();

    Ok(Json(JsonResponse::ok(HealthResponse {
        healthy: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
        stats: StatsData {
            node_count: stats.node_count,
            edge_count: stats.edge_count,
            nodes_by_kind,
            edges_by_relation,
            db_size_bytes: db_size,
        },
    })))
}

async fn stats(State(state): State<AppState>) -> AppResult<Json<JsonResponse<StatsData>>> {
    let stats = state.storage.stats()?;
    let db_size = std::fs::metadata(state.storage.path())
        .map(|m| m.len())
        .unwrap_or(0);

    let nodes_by_kind = stats
        .node_counts_by_kind
        .into_iter()
        .map(|(k, v)| (format!("{:?}", k), v))
        .collect();

    let edges_by_relation = stats
        .edge_counts_by_relation
        .into_iter()
        .map(|(r, v)| (format!("{:?}", r), v))
        .collect();

    Ok(Json(JsonResponse::ok(StatsData {
        node_count: stats.node_count,
        edge_count: stats.edge_count,
        nodes_by_kind,
        edges_by_relation,
        db_size_bytes: db_size,
    })))
}

#[derive(Deserialize)]
struct ListNodesQuery {
    kind: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Serialize)]
struct NodeData {
    id: String,
    kind: String,
    title: String,
    body: String,
    tags: Vec<String>,
    importance: f32,
    source_agent: String,
    edge_count: usize,
}

async fn list_nodes(
    State(state): State<AppState>,
    Query(query): Query<ListNodesQuery>,
) -> AppResult<Json<JsonResponse<Vec<NodeData>>>> {
    let mut filter = NodeFilter::new();

    if let Some(limit) = query.limit {
        filter = filter.with_limit(limit);
    }

    if let Some(offset) = query.offset {
        filter = filter.with_offset(offset);
    }

    if let Some(tag) = query.tag {
        filter = filter.with_tags(vec![tag]);
    }

    if let Some(kind_str) = query.kind {
        let kind = match kind_str.to_lowercase().as_str() {
            "fact" => NodeKind::Fact,
            "decision" => NodeKind::Decision,
            "event" => NodeKind::Event,
            "observation" => NodeKind::Observation,
            "pattern" => NodeKind::Pattern,
            "agent" => NodeKind::Agent,
            "goal" => NodeKind::Goal,
            "preference" => NodeKind::Preference,
            _ => return Err(anyhow::anyhow!("Invalid NodeKind: {}", kind_str).into()),
        };
        filter = filter.with_kinds(vec![kind]);
    }

    let nodes = state.storage.list_nodes(filter)?;

    let node_data: Vec<_> = nodes
        .iter()
        .map(|n| {
            let outgoing = state.storage.edges_from(n.id).unwrap_or_default();
            let incoming = state.storage.edges_to(n.id).unwrap_or_default();
            let edge_count = outgoing.len() + incoming.len();

            NodeData {
                id: n.id.to_string(),
                kind: format!("{:?}", n.kind),
                title: n.data.title.clone(),
                body: n.data.body.clone(),
                tags: n.data.tags.clone(),
                importance: n.importance,
                source_agent: n.source.agent.clone(),
                edge_count,
            }
        })
        .collect();

    Ok(Json(JsonResponse::ok(node_data)))
}

async fn get_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let node_id: uuid::Uuid = id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

    let node = state
        .storage
        .get_node(node_id)?
        .ok_or_else(|| anyhow::anyhow!("Node not found"))?;

    let outgoing = state.storage.edges_from(node.id)?;
    let incoming = state.storage.edges_to(node.id)?;

    let node_data = NodeData {
        id: node.id.to_string(),
        kind: format!("{:?}", node.kind),
        title: node.data.title.clone(),
        body: node.data.body.clone(),
        tags: node.data.tags.clone(),
        importance: node.importance,
        source_agent: node.source.agent.clone(),
        edge_count: outgoing.len() + incoming.len(),
    };

    Ok(Json(JsonResponse::ok(node_data)))
}

#[derive(Deserialize)]
struct NeighborQuery {
    depth: Option<u32>,
    direction: Option<String>,
}

async fn node_neighbors(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<NeighborQuery>,
) -> AppResult<impl IntoResponse> {
    let node_id: uuid::Uuid = id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

    let depth = query.depth.unwrap_or(1);

    // neighborhood() uses Both direction internally; for filtered direction
    // we use traverse directly
    let subgraph = if let Some(ref dir) = query.direction {
        let direction = match dir.to_lowercase().as_str() {
            "outgoing" => cortex_core::TraversalDirection::Outgoing,
            "incoming" => cortex_core::TraversalDirection::Incoming,
            _ => cortex_core::TraversalDirection::Both,
        };
        state.graph_engine.traverse(cortex_core::TraversalRequest {
            start: vec![node_id],
            max_depth: Some(depth),
            direction,
            include_start: true,
            strategy: cortex_core::TraversalStrategy::Bfs,
            ..Default::default()
        })?
    } else {
        state.graph_engine.neighborhood(node_id, depth)?
    };

    let nodes: Vec<_> = subgraph
        .nodes
        .values()
        .map(|n| {
            let outgoing = state.storage.edges_from(n.id).unwrap_or_default();
            let incoming = state.storage.edges_to(n.id).unwrap_or_default();

            NodeData {
                id: n.id.to_string(),
                kind: format!("{:?}", n.kind),
                title: n.data.title.clone(),
                body: n.data.body.clone(),
                tags: n.data.tags.clone(),
                importance: n.importance,
                source_agent: n.source.agent.clone(),
                edge_count: outgoing.len() + incoming.len(),
            }
        })
        .collect();

    Ok(Json(JsonResponse::ok(nodes)))
}

async fn get_edge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let edge_id: uuid::Uuid = id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

    let edge = state
        .storage
        .get_edge(edge_id)?
        .ok_or_else(|| anyhow::anyhow!("Edge not found"))?;

    #[derive(Serialize)]
    struct EdgeData {
        id: String,
        from_id: String,
        to_id: String,
        relation: String,
        weight: f32,
    }

    let edge_data = EdgeData {
        id: edge.id.to_string(),
        from_id: edge.from.to_string(),
        to_id: edge.to.to_string(),
        relation: format!("{:?}", edge.relation),
        weight: edge.weight,
    };

    Ok(Json(JsonResponse::ok(edge_data)))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<impl IntoResponse> {
    let embedding = state.embedding_service.embed(&query.q)?;
    let limit = query.limit.unwrap_or(10);

    let index = state.vector_index.read().unwrap();
    let results = index.search(&embedding, limit, None)?;
    drop(index);

    let search_results: Vec<_> = results
        .iter()
        .filter_map(|r| {
            state
                .storage
                .get_node(r.node_id)
                .ok()
                .flatten()
                .map(|node| {
                    let outgoing = state.storage.edges_from(node.id).unwrap_or_default();
                    let incoming = state.storage.edges_to(node.id).unwrap_or_default();

                    serde_json::json!({
                        "node": NodeData {
                            id: node.id.to_string(),
                            kind: format!("{:?}", node.kind),
                            title: node.data.title.clone(),
                            body: node.data.body.clone(),
                            tags: node.data.tags.clone(),
                            importance: node.importance,
                            source_agent: node.source.agent.clone(),
                            edge_count: outgoing.len() + incoming.len(),
                        },
                        "score": r.score
                    })
                })
        })
        .collect();

    Ok(Json(JsonResponse::ok(search_results)))
}

async fn graph_viz() -> Html<&'static str> {
    Html(GRAPH_VIZ_HTML)
}

#[derive(Serialize)]
struct GraphExport {
    nodes: Vec<NodeData>,
    edges: Vec<EdgeExport>,
}

#[derive(Serialize)]
struct EdgeExport {
    id: String,
    from: String,
    to: String,
    relation: String,
    weight: f32,
}

async fn graph_export(State(state): State<AppState>) -> AppResult<Json<JsonResponse<GraphExport>>> {
    let nodes = state.storage.list_nodes(NodeFilter::new().with_limit(1000))?;

    // Single pass: collect edges and track edge counts simultaneously
    let mut edges = Vec::new();
    let mut edge_counts: HashMap<uuid::Uuid, usize> = HashMap::new();

    for node in &nodes {
        let outgoing = state.storage.edges_from(node.id)?;
        let incoming_count = state.storage.edges_to(node.id)?.len();
        edge_counts.insert(node.id, outgoing.len() + incoming_count);
        for edge in outgoing {
            edges.push(EdgeExport {
                id: edge.id.to_string(),
                from: edge.from.to_string(),
                to: edge.to.to_string(),
                relation: format!("{:?}", edge.relation),
                weight: edge.weight,
            });
        }
    }

    let node_data: Vec<_> = nodes
        .iter()
        .map(|n| NodeData {
            id: n.id.to_string(),
            kind: format!("{:?}", n.kind),
            title: n.data.title.clone(),
            body: n.data.body.clone(),
            tags: n.data.tags.clone(),
            importance: n.importance,
            source_agent: n.source.agent.clone(),
            edge_count: edge_counts.get(&n.id).copied().unwrap_or(0),
        })
        .collect();

    Ok(Json(JsonResponse::ok(GraphExport {
        nodes: node_data,
        edges,
    })))
}

async fn auto_linker_status(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let linker = state.auto_linker.read().unwrap();
    let metrics = linker.metrics();

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "cycles": metrics.cycles,
        "nodes_processed": metrics.nodes_processed,
        "edges_created": metrics.edges_created,
        "edges_pruned": metrics.edges_pruned,
        "backlog_size": metrics.backlog_size,
    }))))
}

async fn trigger_auto_link(
    State(state): State<AppState>,
) -> AppResult<impl IntoResponse> {
    let mut linker = state.auto_linker.write().unwrap();
    linker.run_cycle()?;

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "message": "Auto-link cycle triggered successfully"
    }))))
}

#[derive(Deserialize)]
struct BriefingQuery {
    compact: Option<bool>,
}

#[derive(Serialize)]
struct BriefingSectionData {
    title: String,
    nodes: Vec<NodeData>,
}

#[derive(Serialize)]
struct BriefingData {
    agent_id: String,
    generated_at: String,
    nodes_consulted: usize,
    sections: Vec<BriefingSectionData>,
    rendered: String,
    cached: bool,
}

async fn get_briefing(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Query(query): Query<BriefingQuery>,
) -> AppResult<Json<JsonResponse<BriefingData>>> {
    let compact = query.compact.unwrap_or(false);

    let briefing = state.briefing_engine.generate(&agent_id)?;
    let rendered = state.briefing_engine.render(&briefing, compact);

    let sections: Vec<BriefingSectionData> = briefing
        .sections
        .iter()
        .map(|s| {
            let nodes = s
                .nodes
                .iter()
                .map(|n| {
                    let outgoing = state.storage.edges_from(n.id).unwrap_or_default();
                    let incoming = state.storage.edges_to(n.id).unwrap_or_default();
                    NodeData {
                        id: n.id.to_string(),
                        kind: format!("{:?}", n.kind),
                        title: n.data.title.clone(),
                        body: n.data.body.clone(),
                        tags: n.data.tags.clone(),
                        importance: n.importance,
                        source_agent: n.source.agent.clone(),
                        edge_count: outgoing.len() + incoming.len(),
                    }
                })
                .collect();
            BriefingSectionData {
                title: s.title.clone(),
                nodes,
            }
        })
        .collect();

    Ok(Json(JsonResponse::ok(BriefingData {
        agent_id: briefing.agent_id.clone(),
        generated_at: briefing.generated_at.to_rfc3339(),
        nodes_consulted: briefing.nodes_consulted,
        sections,
        rendered,
        cached: briefing.cached,
    })))
}
