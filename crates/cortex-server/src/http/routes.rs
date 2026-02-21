use super::{prompts, rollback, selection, AppResult, AppState, JsonResponse, GRAPH_VIZ_HTML};
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Json},
    routing::{get, post, put},
    Router,
};
use cortex_core::{apply_score_decay, Edge, EdgeProvenance, NodeFilter, NodeKind, Relation, Source, *};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/nodes", get(list_nodes).post(create_node))
        .route("/nodes/:id", get(get_node).delete(delete_node).patch(patch_node))
        .route("/nodes/:id/neighbors", get(node_neighbors))
        .route("/edges", post(create_edge))
        .route("/edges/:id", get(get_edge))
        .route("/search", get(search))
        .route("/search/hybrid", get(hybrid_search))
        .route("/viz", get(graph_viz))
        .route("/graph/viz", get(graph_viz))
        .route("/graph/export", get(graph_export))
        .route("/auto-linker/status", get(auto_linker_status))
        .route("/auto-linker/trigger", post(trigger_auto_link))
        .route("/briefing/:agent_id", get(get_briefing))
        .route("/agents/:name/prompts", get(list_agent_prompts))
        .route(
            "/agents/:name/prompts/:slug",
            put(bind_prompt).delete(unbind_prompt),
        )
        .route("/agents/:name/resolved-prompt", get(resolved_prompt))
        // Semantic-aware prompt selection (issue #22)
        .route("/agents/:name/active-variant", get(selection::active_variant))
        .route("/agents/:name/variant-history", get(selection::variant_history))
        .route("/agents/:name/observe", post(selection::record_observation))
        // Prompt versioning + inheritance API
        .route(
            "/prompts",
            get(prompts::list_prompts).post(prompts::create_prompt),
        )
        .route("/prompts/:slug/latest", get(prompts::get_latest))
        .route(
            "/prompts/:slug/versions",
            get(prompts::list_versions).post(prompts::create_version),
        )
        .route("/prompts/:slug/versions/:version", get(prompts::get_version))
        .route("/prompts/:slug/branch", post(prompts::create_branch))
        .route("/prompts/:slug/performance", get(selection::prompt_performance))
        // Automatic rollback on performance degradation (issue #23)
        .route("/prompts/:slug/deploy", post(rollback::deploy_prompt))
        .route("/prompts/:slug/rollback-status", get(rollback::rollback_status))
        .route("/prompts/:slug/unquarantine", post(rollback::unquarantine_prompt))
        .route(
            "/prompts/:slug/versions/:version/performance",
            get(selection::version_performance),
        )
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
        let kind = NodeKind::new(&kind_str.to_lowercase())
            .map_err(|e| anyhow::anyhow!("Invalid NodeKind: {}", e))?;
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

#[derive(Deserialize)]
struct CreateNodeBody {
    kind: Option<String>,
    title: String,
    body: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f32>,
    source_agent: Option<String>,
}

async fn create_node(
    State(state): State<AppState>,
    Json(body): Json<CreateNodeBody>,
) -> AppResult<impl IntoResponse> {
    let kind_str = body.kind.as_deref().unwrap_or("fact");
    let kind = NodeKind::new(kind_str).map_err(|e| anyhow::anyhow!("Invalid kind: {}", e))?;
    let importance = body.importance.unwrap_or(0.5);
    let tags = body.tags.unwrap_or_default();
    let source_agent = body.source_agent.unwrap_or_else(|| "http".to_string());
    let node_body = body.body.unwrap_or_else(|| body.title.clone());

    let mut node = Node::new(
        kind,
        body.title.clone(),
        node_body.clone(),
        Source {
            agent: source_agent,
            session: None,
            channel: None,
        },
        importance,
    );
    node.data.tags = tags;

    // Generate embedding
    let embedding = state
        .embedding_service
        .embed(&format!("{} {}", node.data.title, node.data.body))?;

    // Store node
    state.storage.put_node(&node)?;

    // Index embedding
    {
        let mut index = state.vector_index.write().unwrap();
        index.insert(node.id, &embedding)?;
    }

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "id": node.id.to_string(),
        "title": node.data.title,
        "kind": kind_str,
    }))))
}

#[derive(Deserialize)]
struct CreateEdgeBody {
    from_id: String,
    to_id: String,
    relation: Option<String>,
    weight: Option<f32>,
}

async fn create_edge(
    State(state): State<AppState>,
    Json(body): Json<CreateEdgeBody>,
) -> AppResult<impl IntoResponse> {
    let from: uuid::Uuid = body
        .from_id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid from_id UUID"))?;
    let to: uuid::Uuid = body
        .to_id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid to_id UUID"))?;
    let relation_str = body.relation.as_deref().unwrap_or("relates-to");
    let relation =
        Relation::new(relation_str).map_err(|e| anyhow::anyhow!("Invalid relation: {}", e))?;
    let weight = body.weight.unwrap_or(1.0);

    let edge = Edge {
        id: uuid::Uuid::now_v7(),
        from,
        to,
        relation: relation.clone(),
        weight,
        provenance: EdgeProvenance::Manual {
            created_by: "http".to_string(),
        },
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    state.storage.put_edge(&edge)?;

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "id": edge.id.to_string(),
        "from": body.from_id,
        "to": body.to_id,
        "relation": relation_str,
    }))))
}

#[derive(Deserialize)]
struct HybridSearchQuery {
    q: String,
    limit: Option<usize>,
    /// Blend weight for temporal freshness in final score.
    /// 0.0 = pure relevance, 1.0 = heavily favour recent nodes.
    recency_bias: Option<f32>,
}

async fn hybrid_search(
    State(state): State<AppState>,
    Query(query): Query<HybridSearchQuery>,
) -> AppResult<impl IntoResponse> {
    let embedding = state.embedding_service.embed(&query.q)?;
    let limit = query.limit.unwrap_or(10);
    let recency_bias = query
        .recency_bias
        .unwrap_or(state.score_decay.recency_weight);

    // Fetch extra candidates for re-ranking.
    let candidate_limit = if state.score_decay.enabled && recency_bias > 0.0 {
        (limit * 3).max(30)
    } else {
        limit * 2
    };

    let index = state.vector_index.read().unwrap();
    let vector_results = index.search(&embedding, candidate_limit, None)?;
    drop(index);

    // For hybrid: combine vector scores with graph connectivity, then apply decay.
    let mut scored: Vec<(serde_json::Value, f32)> = vector_results
        .iter()
        .filter_map(|r| {
            state
                .storage
                .get_node(r.node_id)
                .ok()
                .flatten()
                .map(|node| {
                    let edge_count = state.storage.edges_from(node.id).unwrap_or_default().len()
                        + state.storage.edges_to(node.id).unwrap_or_default().len();
                    let graph_boost = (edge_count as f32 * 0.05).min(0.3);
                    let combined = r.score + graph_boost;
                    let final_score =
                        apply_score_decay(&node, combined, &state.score_decay, recency_bias);

                    let value = serde_json::json!({
                        "id": node.id.to_string(),
                        "kind": format!("{:?}", node.kind),
                        "title": node.data.title,
                        "body": node.data.body,
                        "score": final_score,
                        "vector_score": r.score,
                        "graph_boost": graph_boost,
                    });
                    (value, final_score)
                })
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    let results: Vec<serde_json::Value> = scored.into_iter().map(|(v, _)| v).collect();

    // Fire-and-forget access recording.
    {
        let node_ids: Vec<NodeId> = results
            .iter()
            .filter_map(|v| v["id"].as_str()?.parse().ok())
            .collect();
        let storage = state.storage.clone();
        tokio::spawn(async move {
            for id in node_ids {
                if let Ok(Some(mut node)) = storage.get_node(id) {
                    node.record_access();
                    let _ = storage.put_node(&node);
                }
            }
        });
    }

    Ok(Json(JsonResponse::ok(results)))
}


async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let node_id: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID"))?;
    state.storage.delete_node(node_id)?;
    Ok(Json(JsonResponse::ok(serde_json::json!({"deleted": id}))))
}

#[derive(Deserialize)]
struct PatchNodeBody {
    kind: Option<String>,
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f32>,
}

async fn patch_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<PatchNodeBody>,
) -> AppResult<impl IntoResponse> {
    let node_id: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID"))?;
    let mut node = state.storage.get_node(node_id)?
        .ok_or_else(|| anyhow::anyhow!("Node not found"))?;

    if let Some(kind_str) = &patch.kind {
        node.kind = cortex_core::NodeKind::new(kind_str)
            .map_err(|e| anyhow::anyhow!("Invalid kind: {}", e))?;
    }
    if let Some(title) = patch.title {
        node.data.title = title;
    }
    if let Some(body) = patch.body {
        node.data.body = body;
    }
    if let Some(tags) = patch.tags {
        node.data.tags = tags;
    }
    if let Some(importance) = patch.importance {
        node.importance = importance;
    }
    node.updated_at = chrono::Utc::now();
    state.storage.put_node(&node)?;

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "id": id,
        "kind": format!("{:?}", node.kind),
        "title": node.data.title,
    }))))
}

async fn get_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let node_id: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

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
    let node_id: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

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
    let edge_id: uuid::Uuid = id.parse().map_err(|_| anyhow::anyhow!("Invalid UUID"))?;

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
    /// Blend weight for temporal freshness in final score.
    /// 0.0 = pure relevance (default), 1.0 = heavily favour recent nodes.
    /// Overrides the configured `score_decay.recency_weight` for this query.
    recency_bias: Option<f32>,
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<impl IntoResponse> {
    let embedding = state.embedding_service.embed(&query.q)?;
    let limit = query.limit.unwrap_or(10);
    let recency_bias = query
        .recency_bias
        .unwrap_or(state.score_decay.recency_weight);

    // Fetch extra candidates so re-ranking by temporal score doesn't cut off
    // good results that vector-rank lower but are fresher / more accessed.
    let candidate_limit = if state.score_decay.enabled && recency_bias > 0.0 {
        (limit * 3).max(30)
    } else {
        limit
    };

    let index = state.vector_index.read().unwrap();
    let results = index.search(&embedding, candidate_limit, None)?;
    drop(index);

    // Pair each raw result with its Node, applying score decay if enabled.
    let mut scored: Vec<(serde_json::Value, f32)> = results
        .iter()
        .filter_map(|r| {
            state
                .storage
                .get_node(r.node_id)
                .ok()
                .flatten()
                .map(|node| {
                    let final_score =
                        apply_score_decay(&node, r.score, &state.score_decay, recency_bias);

                    let outgoing = state.storage.edges_from(node.id).unwrap_or_default();
                    let incoming = state.storage.edges_to(node.id).unwrap_or_default();

                    let value = serde_json::json!({
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
                        "score": final_score,
                        "raw_score": r.score,
                    });
                    (value, final_score)
                })
        })
        .collect();

    // Re-rank by final score (decay may reshuffle from original vector order).
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    let search_results: Vec<serde_json::Value> = scored.into_iter().map(|(v, _)| v).collect();

    // Fire-and-forget: record access for every node returned.
    // Done after results are assembled so it never blocks the response.
    {
        let node_ids: Vec<NodeId> = search_results
            .iter()
            .filter_map(|v| v["node"]["id"].as_str()?.parse().ok())
            .collect();
        let storage = state.storage.clone();
        tokio::spawn(async move {
            for id in node_ids {
                if let Ok(Some(mut node)) = storage.get_node(id) {
                    node.record_access();
                    let _ = storage.put_node(&node);
                }
            }
        });
    }

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
    let nodes = state
        .storage
        .list_nodes(NodeFilter::new().with_limit(1000))?;

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

async fn auto_linker_status(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
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

async fn trigger_auto_link(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
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

// ── Agent ↔ Prompt Bindings ────────────────────────────────────────────────

#[derive(Serialize)]
struct PromptBinding {
    slug: String,
    id: String,
    weight: f32,
    edge_id: String,
}

/// GET /agents/:name/prompts — list all prompts bound to an agent, ordered by weight desc
async fn list_agent_prompts(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    let agent_kind = cortex_core::kinds::defaults::agent();
    let uses_rel = cortex_core::relations::defaults::uses();

    let agent = super::find_by_title(&state.storage, &agent_kind, &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let edges = state.storage.edges_from(agent.id)?;
    let mut bindings: Vec<PromptBinding> = edges
        .into_iter()
        .filter(|e| e.relation == uses_rel)
        .filter_map(|e| {
            state
                .storage
                .get_node(e.to)
                .ok()
                .flatten()
                .map(|prompt| PromptBinding {
                    slug: prompt.data.title.clone(),
                    id: prompt.id.to_string(),
                    weight: e.weight,
                    edge_id: e.id.to_string(),
                })
        })
        .collect();

    bindings.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

    Ok(Json(JsonResponse::ok(bindings)))
}

#[derive(Deserialize)]
struct BindPromptBody {
    weight: Option<f32>,
}

/// PUT /agents/:name/prompts/:slug — bind or update an agent→prompt edge
async fn bind_prompt(
    State(state): State<AppState>,
    Path((name, slug)): Path<(String, String)>,
    Json(body): Json<BindPromptBody>,
) -> AppResult<impl IntoResponse> {
    let agent_kind = cortex_core::kinds::defaults::agent();
    let prompt_kind = cortex_core::kinds::defaults::prompt();
    let uses_rel = cortex_core::relations::defaults::uses();

    let agent = super::find_by_title(&state.storage, &agent_kind, &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found. Create it first via POST /nodes with kind=agent.", name))?;

    let prompt = super::find_by_title(&state.storage, &prompt_kind, &slug)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}' not found. Create it first via POST /nodes with kind=prompt.", slug))?;

    let weight = body.weight.unwrap_or(1.0).clamp(0.0, 1.0);

    // Remove any existing uses edge between this agent and prompt
    let existing = state.storage.edges_between(agent.id, prompt.id)?;
    for edge in existing.iter().filter(|e| e.relation == uses_rel) {
        state.storage.delete_edge(edge.id)?;
    }

    // Create the new (or replacement) edge
    let edge = Edge {
        id: uuid::Uuid::now_v7(),
        from: agent.id,
        to: prompt.id,
        relation: uses_rel,
        weight,
        provenance: EdgeProvenance::Manual {
            created_by: "http".to_string(),
        },
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.storage.put_edge(&edge)?;

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "agent": name,
        "prompt": slug,
        "weight": weight,
        "edge_id": edge.id.to_string(),
    }))))
}

/// DELETE /agents/:name/prompts/:slug — unbind a prompt from an agent
async fn unbind_prompt(
    State(state): State<AppState>,
    Path((name, slug)): Path<(String, String)>,
) -> AppResult<impl IntoResponse> {
    let agent_kind = cortex_core::kinds::defaults::agent();
    let prompt_kind = cortex_core::kinds::defaults::prompt();
    let uses_rel = cortex_core::relations::defaults::uses();

    let agent = super::find_by_title(&state.storage, &agent_kind, &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let prompt = super::find_by_title(&state.storage, &prompt_kind, &slug)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}' not found", slug))?;

    let existing = state.storage.edges_between(agent.id, prompt.id)?;
    let to_delete: Vec<_> = existing
        .iter()
        .filter(|e| e.relation == uses_rel)
        .collect();

    if to_delete.is_empty() {
        return Err(anyhow::anyhow!(
            "No 'uses' binding found between agent '{}' and prompt '{}'",
            name,
            slug
        )
        .into());
    }

    for edge in to_delete {
        state.storage.delete_edge(edge.id)?;
    }

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "agent": name,
        "prompt": slug,
        "unbound": true,
    }))))
}

#[derive(Serialize)]
struct ResolvedPromptData {
    agent: String,
    prompts_consulted: usize,
    bindings: Vec<PromptBinding>,
    resolved: String,
}

/// GET /agents/:name/resolved-prompt — merge all bound prompts in weight order
async fn resolved_prompt(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<impl IntoResponse> {
    let agent_kind = cortex_core::kinds::defaults::agent();
    let uses_rel = cortex_core::relations::defaults::uses();

    let agent = super::find_by_title(&state.storage, &agent_kind, &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let edges = state.storage.edges_from(agent.id)?;

    // Collect (edge, prompt_node) pairs for uses edges, sorted by weight desc
    let mut prompt_pairs: Vec<(cortex_core::Edge, Node)> = edges
        .into_iter()
        .filter(|e| e.relation == uses_rel)
        .filter_map(|e| {
            state
                .storage
                .get_node(e.to)
                .ok()
                .flatten()
                .map(|prompt| (e, prompt))
        })
        .collect();

    prompt_pairs
        .sort_by(|a, b| b.0.weight.partial_cmp(&a.0.weight).unwrap_or(std::cmp::Ordering::Equal));

    let bindings: Vec<PromptBinding> = prompt_pairs
        .iter()
        .map(|(e, p)| PromptBinding {
            slug: p.data.title.clone(),
            id: p.id.to_string(),
            weight: e.weight,
            edge_id: e.id.to_string(),
        })
        .collect();

    // Merge prompt bodies: highest weight = base identity, rest appended as overlays
    let mut resolved = String::new();
    for (i, (edge, prompt)) in prompt_pairs.iter().enumerate() {
        if i == 0 {
            resolved.push_str(&format!("# {}\n\n", prompt.data.title));
        } else {
            resolved.push_str(&format!("\n\n---\n\n# {} (overlay, weight: {:.2})\n\n", prompt.data.title, edge.weight));
        }
        resolved.push_str(&prompt.data.body);
    }

    Ok(Json(JsonResponse::ok(ResolvedPromptData {
        agent: name,
        prompts_consulted: bindings.len(),
        bindings,
        resolved,
    })))
}
