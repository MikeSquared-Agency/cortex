/// Continuous Semantic-Aware Prompt Selection (issue #22)
///
/// Endpoints:
///   GET  /agents/:name/active-variant     — score all variants, epsilon-greedy select
///   GET  /agents/:name/variant-history    — timeline of swap/performance observations
///   POST /agents/:name/observe            — record performance, update edge weight
///   GET  /prompts/:slug/performance       — aggregate stats across all contexts
use super::{find_by_title, AppResult, AppState, JsonResponse};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Json},
};
use cortex_core::{
    kinds::defaults as kinds, prompt::selection as sel, relations::defaults as rels, Edge,
    EdgeProvenance, Node, Source, *,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

// ── GET /agents/:name/active-variant ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct ActiveVariantQuery {
    #[serde(default = "default_half")]
    sentiment: f32,
    #[serde(default = "default_casual")]
    task_type: String,
    #[serde(default)]
    correction_rate: f32,
    #[serde(default)]
    topic_shift: f32,
    #[serde(default = "default_half")]
    energy: f32,
    /// Exploration rate for epsilon-greedy (0.0 = always exploit, 1.0 = always random)
    #[serde(default = "default_epsilon")]
    epsilon: f32,
}

fn default_half() -> f32 {
    0.5
}
fn default_casual() -> String {
    "casual".to_string()
}
fn default_epsilon() -> f32 {
    0.2
}

#[derive(Serialize, Clone)]
struct VariantScore {
    id: String,
    slug: String,
    edge_weight: f32,
    /// Normalised context fit score (0–1). Equal to `edge_weight` when no context_weights set.
    context_score: f32,
    total_score: f32,
}

#[derive(Serialize)]
struct ActiveVariantResponse {
    agent: String,
    selected: Option<VariantScore>,
    current_variant_id: Option<String>,
    swap_recommended: bool,
    epsilon: f32,
    signals: serde_json::Value,
    all_variants: Vec<VariantScore>,
}

pub async fn active_variant(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<ActiveVariantQuery>,
) -> AppResult<impl IntoResponse> {
    let signals = sel::ContextSignals {
        sentiment: q.sentiment,
        task_type: q.task_type,
        correction_rate: q.correction_rate,
        topic_shift: q.topic_shift,
        energy: q.energy,
    };

    let agent = find_by_title(&state.storage, &kinds::agent(), &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let current_variant_id: Option<String> = agent
        .data
        .metadata
        .get("active_variant_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Score all bound variants in a single pass, using get_signal (no per-variant HashMap alloc)
    let uses_rel = rels::uses();
    let edges = state.storage.edges_from(agent.id)?;
    let mut scores: Vec<VariantScore> = edges
        .into_iter()
        .filter(|e| e.relation == uses_rel)
        .filter_map(|e| {
            state.storage.get_node(e.to).ok().flatten().map(|prompt| {
                let cw = prompt.data.metadata.get("context_weights").cloned();
                // context_fit returns None when no weights set — fall back to edge_weight
                let fit = sel::context_fit(cw.as_ref(), &signals);
                let total = match fit {
                    None => e.weight,
                    Some(f) => (0.5 * e.weight + 0.5 * f).clamp(0.0, 1.0),
                };
                VariantScore {
                    id: prompt.id.to_string(),
                    slug: prompt.data.title.clone(),
                    edge_weight: e.weight,
                    context_score: fit.unwrap_or(e.weight),
                    total_score: total,
                }
            })
        })
        .collect();

    if scores.is_empty() {
        return Ok(Json(JsonResponse::ok(ActiveVariantResponse {
            agent: name,
            selected: None,
            current_variant_id,
            swap_recommended: false,
            epsilon: q.epsilon,
            signals: serde_json::to_value(&signals).unwrap_or_default(),
            all_variants: vec![],
        })));
    }

    // Epsilon-greedy: determine selected id before sorting
    let epsilon = q.epsilon.clamp(0.0, 1.0);
    let mut rng = rand::thread_rng();
    let selected_idx = if rng.gen::<f32>() < epsilon {
        // Explore: uniform random choice
        rng.gen_range(0..scores.len())
    } else {
        // Exploit: pick highest total_score
        scores
            .iter()
            .enumerate()
            .max_by(|a, b| {
                a.1.total_score
                    .partial_cmp(&b.1.total_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    // Capture selected before sort invalidates the index
    let selected_variant = scores[selected_idx].clone();

    // Sort all_variants by total_score desc for presentation
    scores.sort_by(|a, b| {
        b.total_score
            .partial_cmp(&a.total_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let swap_recommended = current_variant_id
        .as_deref()
        .map(|cur| cur != selected_variant.id)
        .unwrap_or(true);

    Ok(Json(JsonResponse::ok(ActiveVariantResponse {
        agent: name,
        swap_recommended,
        current_variant_id,
        epsilon,
        signals: serde_json::to_value(&signals).unwrap_or_default(),
        selected: Some(selected_variant),
        all_variants: scores,
    })))
}

// ── GET /agents/:name/variant-history ────────────────────────────────────────

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    limit: usize,
}

fn default_history_limit() -> usize {
    20
}

pub async fn variant_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> AppResult<impl IntoResponse> {
    let agent = find_by_title(&state.storage, &kinds::agent(), &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let performed_rel = rels::performed();

    // Collect raw nodes, sort by created_at (proper DateTime comparison), then truncate
    // before building JSON — avoids serialising observations we'll discard.
    let mut raw_nodes: Vec<Node> = state
        .storage
        .edges_from(agent.id)?
        .into_iter()
        .filter(|e| e.relation == performed_rel)
        .filter_map(|e| state.storage.get_node(e.to).ok().flatten())
        .collect();

    raw_nodes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    raw_nodes.truncate(q.limit);

    let observations: Vec<serde_json::Value> = raw_nodes
        .iter()
        .map(|obs| {
            let meta = &obs.data.metadata;
            serde_json::json!({
                "id": obs.id.to_string(),
                "type": meta.get("observation_type").and_then(|v| v.as_str()).unwrap_or("performance"),
                "variant_id": meta.get("variant_id").and_then(|v| v.as_str()),
                "variant_slug": meta.get("variant_slug").and_then(|v| v.as_str()),
                "old_variant_id": meta.get("old_variant_id").and_then(|v| v.as_str()),
                "old_variant_slug": meta.get("old_variant_slug").and_then(|v| v.as_str()),
                "observation_score": meta.get("observation_score").and_then(|v| v.as_f64()),
                "sentiment_score": meta.get("sentiment_score").and_then(|v| v.as_f64()),
                "task_outcome": meta.get("task_outcome").and_then(|v| v.as_str()),
                "token_cost": meta.get("token_cost").and_then(|v| v.as_i64()),
                "trigger_signal": meta.get("trigger_signal").and_then(|v| v.as_str()),
                "created_at": obs.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(JsonResponse::ok(observations)))
}

// ── POST /agents/:name/observe ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ObserveBody {
    /// UUID of the prompt variant node
    pub variant_id: String,
    /// Slug/title of the prompt variant (for display)
    pub variant_slug: String,
    /// Observed sentiment score: 0.0–1.0
    #[serde(default = "default_half")]
    pub sentiment_score: f32,
    /// Number of corrections the user made
    #[serde(default)]
    pub correction_count: u32,
    /// Outcome: success | partial | failure | unknown
    #[serde(default = "default_unknown")]
    pub task_outcome: String,
    /// Token cost of the interaction (informational)
    pub token_cost: Option<u32>,
    /// The context signals active during this interaction
    pub context_signals: Option<sel::ContextSignals>,
}

fn default_unknown() -> String {
    "unknown".to_string()
}

#[derive(Serialize)]
struct ObserveResponse {
    observation_id: String,
    variant_id: String,
    variant_slug: String,
    observation_score: f32,
    old_edge_weight: f32,
    new_edge_weight: f32,
}

pub async fn record_observation(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ObserveBody>,
) -> AppResult<impl IntoResponse> {
    let agent = find_by_title(&state.storage, &kinds::agent(), &name)?
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", name))?;

    let variant_uuid: uuid::Uuid = body
        .variant_id
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid variant_id UUID"))?;

    // Compute observation score
    let obs_score = sel::observation_score(
        body.sentiment_score,
        body.correction_count,
        &body.task_outcome,
    );

    // Build observation node
    let mut obs_node = Node::new(
        kinds::observation(),
        format!("{}: Performance for {}", body.task_outcome, body.variant_slug),
        format!(
            "Sentiment: {:.2}, Corrections: {}, Outcome: {}, Token cost: {}",
            body.sentiment_score,
            body.correction_count,
            body.task_outcome,
            body.token_cost
                .map(|c| c.to_string())
                .as_deref()
                .unwrap_or("n/a"),
        ),
        Source {
            agent: name.clone(),
            session: None,
            channel: None,
        },
        obs_score,
    );

    obs_node.data.metadata.insert(
        "observation_type".into(),
        serde_json::Value::String("performance".into()),
    );
    obs_node
        .data
        .metadata
        .insert("variant_id".into(), serde_json::Value::String(body.variant_id.clone()));
    obs_node
        .data
        .metadata
        .insert("variant_slug".into(), serde_json::Value::String(body.variant_slug.clone()));
    obs_node
        .data
        .metadata
        .insert("sentiment_score".into(), serde_json::json!(body.sentiment_score));
    obs_node
        .data
        .metadata
        .insert("correction_count".into(), serde_json::json!(body.correction_count));
    obs_node.data.metadata.insert(
        "task_outcome".into(),
        serde_json::Value::String(body.task_outcome.clone()),
    );
    obs_node
        .data
        .metadata
        .insert("observation_score".into(), serde_json::json!(obs_score));
    if let Some(tc) = body.token_cost {
        obs_node
            .data
            .metadata
            .insert("token_cost".into(), serde_json::json!(tc));
    }
    if let Some(ref signals) = body.context_signals {
        obs_node.data.metadata.insert(
            "context_signals".into(),
            serde_json::to_value(signals).unwrap_or_default(),
        );
    }

    state.storage.put_node(&obs_node)?;

    // Batch both new edges in a single write transaction
    let new_edges = vec![
        Edge {
            id: uuid::Uuid::now_v7(),
            from: agent.id,
            to: obs_node.id,
            relation: rels::performed(),
            weight: 1.0,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
        Edge {
            id: uuid::Uuid::now_v7(),
            from: obs_node.id,
            to: variant_uuid,
            relation: rels::informed_by(),
            weight: 1.0,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
    ];
    state.storage.put_edges_batch(&new_edges)?;

    // Atomically update the uses edge weight (single write transaction)
    let uses_rel = rels::uses();
    let old_weight = state
        .storage
        .edges_between(agent.id, variant_uuid)?
        .iter()
        .find(|e| e.relation == uses_rel)
        .map(|e| e.weight)
        .unwrap_or(1.0);

    let new_weight = state.storage.update_edge_weight_atomic(
        agent.id,
        variant_uuid,
        &uses_rel,
        |w| sel::update_edge_weight(w, obs_score),
    )?;

    // Determine if this is a variant swap
    let current_active = agent
        .data
        .metadata
        .get("active_variant_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let is_swap = current_active.as_deref() != Some(&body.variant_id);

    // Record a swap observation if the active variant changed.
    // Guard against corrupted/non-UUID active_variant_id with if-let.
    if is_swap {
        if let Some(ref old_id) = current_active {
            if let Ok(old_uuid) = old_id.parse::<uuid::Uuid>() {
                let old_slug = state
                    .storage
                    .get_node(old_uuid)
                    .ok()
                    .flatten()
                    .map(|n| n.data.title.clone())
                    .unwrap_or_default();

                let mut swap_obs = Node::new(
                    kinds::observation(),
                    format!("Swap: {} → {}", old_slug, body.variant_slug),
                    format!(
                        "Variant swapped from '{}' to '{}' after {} outcome.",
                        old_slug, body.variant_slug, body.task_outcome
                    ),
                    Source { agent: name.clone(), session: None, channel: None },
                    0.5,
                );
                swap_obs.data.metadata.insert(
                    "observation_type".into(),
                    serde_json::Value::String("swap".into()),
                );
                swap_obs.data.metadata.insert(
                    "old_variant_id".into(),
                    serde_json::Value::String(old_id.clone()),
                );
                swap_obs.data.metadata.insert(
                    "old_variant_slug".into(),
                    serde_json::Value::String(old_slug),
                );
                swap_obs.data.metadata.insert(
                    "new_variant_id".into(),
                    serde_json::Value::String(body.variant_id.clone()),
                );
                swap_obs.data.metadata.insert(
                    "new_variant_slug".into(),
                    serde_json::Value::String(body.variant_slug.clone()),
                );
                swap_obs.data.metadata.insert(
                    "trigger_signal".into(),
                    serde_json::Value::String(body.task_outcome.clone()),
                );
                state.storage.put_node(&swap_obs)?;

                state.storage.put_edge(&Edge {
                    id: uuid::Uuid::now_v7(),
                    from: agent.id,
                    to: swap_obs.id,
                    relation: rels::performed(),
                    weight: 1.0,
                    provenance: EdgeProvenance::Manual { created_by: name.clone() },
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })?;
            } else {
                log::warn!(
                    "agent '{}' has non-UUID active_variant_id '{}'; skipping swap observation",
                    name,
                    old_id
                );
            }
        }
    }

    // Update agent node metadata with new active_variant_id
    let mut updated_agent = agent.clone();
    updated_agent.data.metadata.insert(
        "active_variant_id".into(),
        serde_json::Value::String(body.variant_id.clone()),
    );
    updated_agent.updated_at = chrono::Utc::now();
    state.storage.put_node(&updated_agent)?;

    Ok(Json(JsonResponse::ok(ObserveResponse {
        observation_id: obs_node.id.to_string(),
        variant_id: body.variant_id,
        variant_slug: body.variant_slug,
        observation_score: obs_score,
        old_edge_weight: old_weight,
        new_edge_weight: new_weight,
    })))
}

// ── GET /prompts/:slug/performance ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct PerformanceQuery {
    #[serde(default = "default_perf_limit")]
    limit: usize,
}

fn default_perf_limit() -> usize {
    50
}

pub async fn prompt_performance(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<PerformanceQuery>,
) -> AppResult<impl IntoResponse> {
    let prompt = find_by_title(&state.storage, &kinds::prompt(), &slug)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}' not found", slug))?;

    // Collect all performance observations linked via obs --[informed_by]--> prompt
    let informed_rel = rels::informed_by();
    let mut all_obs: Vec<Node> = state
        .storage
        .edges_to(prompt.id)?
        .into_iter()
        .filter(|e| e.relation == informed_rel)
        .filter_map(|e| state.storage.get_node(e.from).ok().flatten())
        .filter(|n| {
            n.data
                .metadata
                .get("observation_type")
                .and_then(|v| v.as_str())
                == Some("performance")
        })
        .collect();

    // Sort descending by created_at once
    all_obs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // Compute aggregates over the FULL observation set (not just the truncated window),
    // so avg_score reflects true historical performance — not just the most recent N.
    let total_count = all_obs.len();
    let mut sum_score = 0.0f64;
    let mut sum_sentiment = 0.0f64;
    let mut sum_corrections = 0u64;
    let mut task_outcomes: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for n in &all_obs {
        let meta = &n.data.metadata;
        sum_score += meta.get("observation_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sum_sentiment += meta.get("sentiment_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sum_corrections += meta.get("correction_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let outcome = meta
            .get("task_outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        *task_outcomes.entry(outcome).or_insert(0) += 1;
    }

    let avg_score =
        if total_count > 0 { sum_score / total_count as f64 } else { 0.0 };
    let avg_sentiment =
        if total_count > 0 { sum_sentiment / total_count as f64 } else { 0.0 };
    let avg_corrections =
        if total_count > 0 { sum_corrections as f64 / total_count as f64 } else { 0.0 };

    // Only the detail view is truncated — aggregates remain over the full set
    all_obs.truncate(q.limit);

    // Single pass to build observations JSON (reuses locals from the aggregate loop above)
    let observations: Vec<serde_json::Value> = all_obs
        .iter()
        .map(|n| {
            let meta = &n.data.metadata;
            let score = meta.get("observation_score").and_then(|v| v.as_f64());
            let sentiment = meta.get("sentiment_score").and_then(|v| v.as_f64());
            let corr = meta.get("correction_count").and_then(|v| v.as_u64());
            let outcome = meta.get("task_outcome").and_then(|v| v.as_str());
            let cost = meta.get("token_cost").and_then(|v| v.as_u64());
            serde_json::json!({
                "id": n.id.to_string(),
                "observation_score": score,
                "sentiment_score": sentiment,
                "correction_count": corr,
                "task_outcome": outcome,
                "token_cost": cost,
                "created_at": n.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "slug": slug,
        "prompt_id": prompt.id.to_string(),
        // Aggregates span ALL observations, not just the page window
        "observation_count": total_count,
        "avg_score": avg_score,
        "avg_sentiment": avg_sentiment,
        "avg_correction_count": avg_corrections,
        "task_outcomes": task_outcomes,
        // Detail window limited to q.limit most-recent observations
        "observations_shown": observations.len(),
        "observations": observations,
    }))))
}
