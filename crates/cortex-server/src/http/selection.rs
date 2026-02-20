/// Continuous Semantic-Aware Prompt Selection (issue #22)
/// Usage Tracking & Performance Observations (issue #24)
///
/// Endpoints:
///   GET  /agents/:name/active-variant              — score all variants, epsilon-greedy select
///   GET  /agents/:name/variant-history             — timeline of swap/performance observations
///   POST /agents/:name/observe                     — record performance, update edge weight
///   GET  /prompts/:slug/performance                — aggregate stats across all contexts
///   GET  /prompts/:slug/versions/:v/performance    — aggregate stats for a specific version
use super::{find_by_title, AppResult, AppState, JsonResponse};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Json},
};
use cortex_core::{
    kinds::defaults as kinds,
    prompt::{selection as sel, PromptResolver, RollbackMonitor},
    relations::defaults as rels,
    Edge, EdgeProvenance, Node, Source, Storage,
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
            // Single parse via extract_obs — obs_type and prompt_slug from root fields
            let ex = extract_obs(obs);
            let meta = &obs.data.metadata;

            // variant_slug: body JSON prompt_slug takes priority, then metadata.
            // variant_id: UUID lives in metadata only (#24 body JSON omits it).
            let variant_slug = ex
                .prompt_slug
                .or_else(|| meta.get("variant_slug").and_then(|v| v.as_str()).map(|s| s.to_string()));
            let variant_id =
                meta.get("variant_id").and_then(|v| v.as_str()).map(|s| s.to_string());

            serde_json::json!({
                "id": obs.id.to_string(),
                "type": ex.obs_type,
                "variant_id": variant_id,
                "variant_slug": variant_slug,
                "old_variant_id": meta.get("old_variant_id").and_then(|v| v.as_str()),
                "old_variant_slug": meta.get("old_variant_slug").and_then(|v| v.as_str()),
                "observation_score": ex.score,
                "sentiment_score": ex.sentiment,
                "task_outcome": ex.outcome,
                "token_cost": ex.token_cost,
                "trigger_signal": meta.get("trigger_signal").and_then(|v| v.as_str()),
                "created_at": obs.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(JsonResponse::ok(observations)))
}

// ── POST /agents/:name/observe ────────────────────────────────────────────────

/// Rich body JSON stored inside the observation node (issue #24 schema).
#[derive(Serialize, Deserialize, Default)]
struct ObsBodyJson {
    agent: String,
    prompt_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_version: Option<u32>,
    observation_type: String,
    metrics: ObsMetrics,
    context: ObsContext,
}

#[derive(Serialize, Deserialize, Default)]
struct ObsMetrics {
    correction_count: u32,
    sentiment_score: f32,
    task_completed: bool,
    task_outcome: String,
    observation_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_time_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_satisfaction: Option<f32>,
}

#[derive(Serialize, Deserialize, Default)]
struct ObsContext {
    task_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correction_rate: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic_shift: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    energy: Option<f32>,
}

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
    /// Token cost of the interaction
    pub token_cost: Option<u32>,
    /// Response time in milliseconds
    pub response_time_ms: Option<u32>,
    /// Explicit user satisfaction score (0.0–1.0), if available
    pub user_satisfaction: Option<f32>,
    /// The context signals active during this interaction
    pub context_signals: Option<sel::ContextSignals>,
    /// Topic/domain of the interaction (e.g. "cortex-development")
    pub topic: Option<String>,
    /// Session length in minutes since session start
    pub session_length: Option<u32>,
    /// Number of messages in the session
    pub message_count: Option<u32>,
}

fn default_unknown() -> String {
    "unknown".to_string()
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

    // Normalise inputs: clamp scores to valid range, canonicalise task_outcome
    let sentiment_score = body.sentiment_score.clamp(0.0, 1.0);
    let user_satisfaction = body.user_satisfaction.map(|v| v.clamp(0.0, 1.0));
    let task_outcome = match body.task_outcome.as_str() {
        "success" | "partial" | "failure" | "unknown" => body.task_outcome.clone(),
        _ => "unknown".to_string(),
    };

    // Compute observation score
    let obs_score = sel::observation_score(sentiment_score, body.correction_count, &task_outcome);

    // Try to look up the prompt version from the variant node's body JSON
    let prompt_version: Option<u32> = state
        .storage
        .get_node(variant_uuid)
        .ok()
        .flatten()
        .and_then(|n| serde_json::from_str::<serde_json::Value>(&n.data.body).ok())
        .and_then(|v| v.get("version").and_then(|v| v.as_u64()))
        .map(|v| v as u32);

    let task_type = body
        .context_signals
        .as_ref()
        .map(|s| s.task_type.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Build the rich body JSON (issue #24 schema)
    let obs_body = ObsBodyJson {
        agent: name.clone(),
        prompt_slug: body.variant_slug.clone(),
        prompt_version,
        observation_type: "performance".to_string(),
        metrics: ObsMetrics {
            correction_count: body.correction_count,
            sentiment_score: sentiment_score,
            task_completed: task_outcome == "success",
            task_outcome: task_outcome.clone(),
            observation_score: obs_score,
            token_cost: body.token_cost,
            response_time_ms: body.response_time_ms,
            user_satisfaction: user_satisfaction,
        },
        context: ObsContext {
            task_type,
            topic: body.topic.clone(),
            session_length: body.session_length,
            message_count: body.message_count,
            correction_rate: body.context_signals.as_ref().map(|s| s.correction_rate),
            topic_shift: body.context_signals.as_ref().map(|s| s.topic_shift),
            energy: body.context_signals.as_ref().map(|s| s.energy),
        },
    };
    let body_json_str = serde_json::to_string(&obs_body).unwrap_or_default();

    // Title follows issue #24 schema: obs:<agent>:<timestamp>
    let now = chrono::Utc::now();
    let obs_title = format!("obs:{}:{}", name, now.to_rfc3339());

    let mut obs_node = Node::new(
        kinds::observation(),
        obs_title,
        body_json_str,
        Source {
            agent: name.clone(),
            session: None,
            channel: None,
        },
        obs_score,
    );

    // Keep backward-compat metadata entries so existing variant-history still works
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
        .insert("sentiment_score".into(), serde_json::json!(sentiment_score));
    obs_node
        .data
        .metadata
        .insert("correction_count".into(), serde_json::json!(body.correction_count));
    obs_node.data.metadata.insert(
        "task_outcome".into(),
        serde_json::Value::String(task_outcome.clone()),
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

    // Edges: agent --[performed]--> obs (backward compat)
    //        obs --[informed_by]--> variant (backward compat for performance query)
    //        obs --[observed_with]--> variant (issue #24 naming)
    //        obs --[observed_by]--> agent (issue #24 naming)
    let new_edges = vec![
        Edge {
            id: uuid::Uuid::now_v7(),
            from: agent.id,
            to: obs_node.id,
            relation: rels::performed(),
            weight: 1.0,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: now,
            updated_at: now,
        },
        Edge {
            id: uuid::Uuid::now_v7(),
            from: obs_node.id,
            to: variant_uuid,
            relation: rels::informed_by(),
            weight: 1.0,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: now,
            updated_at: now,
        },
        Edge {
            id: uuid::Uuid::now_v7(),
            from: obs_node.id,
            to: variant_uuid,
            relation: rels::observed_with(),
            weight: obs_score,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: now,
            updated_at: now,
        },
        Edge {
            id: uuid::Uuid::now_v7(),
            from: obs_node.id,
            to: agent.id,
            relation: rels::observed_by(),
            weight: 1.0,
            provenance: EdgeProvenance::Manual { created_by: name.clone() },
            created_at: now,
            updated_at: now,
        },
    ];
    state.storage.put_edges_batch(&new_edges)?;

    // Atomically update the uses edge weight (single write transaction)
    let uses_rel = rels::uses();
    let (old_weight, new_weight) = state.storage.update_edge_weight_atomic(
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

                let swap_body_json = serde_json::to_string(&serde_json::json!({
                    "agent": name,
                    "observation_type": "swap",
                    "old_variant_id": old_id,
                    "old_variant_slug": old_slug,
                    "new_variant_id": body.variant_id,
                    "new_variant_slug": body.variant_slug,
                    "trigger_signal": task_outcome,
                }))
                .unwrap_or_default();

                let mut swap_obs = Node::new(
                    kinds::observation(),
                    format!("obs:{}:{}", name, now.to_rfc3339()),
                    swap_body_json,
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
                    serde_json::Value::String(task_outcome.clone()),
                );
                state.storage.put_node(&swap_obs)?;

                state.storage.put_edge(&Edge {
                    id: uuid::Uuid::now_v7(),
                    from: agent.id,
                    to: swap_obs.id,
                    relation: rels::performed(),
                    weight: 1.0,
                    provenance: EdgeProvenance::Manual { created_by: name.clone() },
                    created_at: now,
                    updated_at: now,
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
    updated_agent.updated_at = now;
    state.storage.put_node(&updated_agent)?;

    // ── Rollback monitor check (issue #23) ─────────────────────────────────
    // Normalise correction_count to a rate (0–1) assuming 5 corrections = rate 1.0.
    let correction_rate = (body.correction_count as f32 / 5.0).min(1.0);
    let rollback_result = RollbackMonitor::new(
        state.storage.clone(),
        state.rollback_config.clone(),
    )
    .process_observation(obs_node.id, variant_uuid, correction_rate, sentiment_score, obs_score)
    .unwrap_or_else(|e| {
        log::warn!("rollback monitor error for variant {}: {}", variant_uuid, e);
        None
    });

    let rollback_info = rollback_result.as_ref().map(|r| {
        serde_json::json!({
            "triggered": true,
            "rollback_node_id": r.rollback_node_id.to_string(),
            "from_version": r.from_version,
            "to_version": r.to_version,
            "trigger": r.trigger.kind_str(),
            "cooldown_hours": r.cooldown_hours,
            "is_quarantined": r.is_quarantined,
        })
    });

    // Fire rollback notification webhooks (issue #23 — notify_on_rollback)
    if let Some(ref rb) = rollback_result {
        for wh in &state.webhooks {
            if wh.events.iter().any(|e| e == "rollback" || e == "*") {
                let payload = serde_json::json!({
                    "event": "prompt.rollback",
                    "agent": name,
                    "from_version": rb.from_version,
                    "to_version": rb.to_version,
                    "trigger": rb.trigger.kind_str(),
                    "cooldown_hours": rb.cooldown_hours,
                    "is_quarantined": rb.is_quarantined,
                    "rollback_node_id": rb.rollback_node_id.to_string(),
                });
                let url = wh.url.clone();
                // Fire-and-forget in background to avoid blocking the response
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    if let Err(e) = client.post(&url).json(&payload).send().await {
                        log::warn!("rollback webhook to {} failed: {}", url, e);
                    }
                });
            }
        }
    }

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "observation_id": obs_node.id.to_string(),
        "variant_id": body.variant_id,
        "variant_slug": body.variant_slug,
        "observation_score": obs_score,
        "old_edge_weight": old_weight,
        "new_edge_weight": new_weight,
        "rollback": rollback_info,
    }))))
}

// ── Shared aggregation helpers ────────────────────────────────────────────────

/// All fields extracted from one observation node — body JSON parsed exactly once,
/// with automatic fallback to legacy metadata for pre-#24 nodes.
struct ExtractedObs {
    /// Root-level observation type ("performance" or "swap").
    obs_type: String,
    /// Prompt slug from body JSON (`prompt_slug`), if present (new-format nodes only).
    prompt_slug: Option<String>,
    score: f64,
    sentiment: f64,
    corrections: u64,
    outcome: String,
    token_cost: Option<u64>,
    response_time_ms: Option<u64>,
    user_satisfaction: Option<f64>,
    /// Parsed `context` sub-object for detail views and context filtering.
    context: Option<serde_json::Value>,
}

/// Parse an observation node's body JSON exactly once, extracting all fields.
/// Falls back to node metadata for legacy nodes that pre-date the #24 schema.
fn extract_obs(n: &Node) -> ExtractedObs {
    // Single parse — reuse `parsed` via `as_ref()` for all field reads,
    // then move it at the very end to extract `context`.
    let parsed = serde_json::from_str::<serde_json::Value>(&n.data.body).ok();
    let meta = &n.data.metadata;

    // Root-level fields (new format only; no metadata fallback needed)
    let obs_type = parsed
        .as_ref()
        .and_then(|b| b.get("observation_type"))
        .and_then(|v| v.as_str())
        .or_else(|| meta.get("observation_type").and_then(|v| v.as_str()))
        .unwrap_or("performance")
        .to_string();
    let prompt_slug = parsed
        .as_ref()
        .and_then(|b| b.get("prompt_slug"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Metrics fields — body JSON takes precedence over legacy metadata
    let m_f64 = |field: &str| -> Option<f64> {
        parsed
            .as_ref()
            .and_then(|b| b.get("metrics"))
            .and_then(|m| m.get(field))
            .and_then(|v| v.as_f64())
            .or_else(|| meta.get(field).and_then(|v| v.as_f64()))
    };
    let m_u64 = |field: &str| -> Option<u64> {
        parsed
            .as_ref()
            .and_then(|b| b.get("metrics"))
            .and_then(|m| m.get(field))
            .and_then(|v| v.as_u64())
            .or_else(|| meta.get(field).and_then(|v| v.as_u64()))
    };
    let m_str_owned = |field: &str| -> Option<String> {
        parsed
            .as_ref()
            .and_then(|b| b.get("metrics"))
            .and_then(|m| m.get(field))
            .and_then(|v| v.as_str())
            .or_else(|| meta.get(field).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    };

    let score = m_f64("observation_score").unwrap_or(0.0);
    let sentiment = m_f64("sentiment_score").unwrap_or(0.0);
    let corrections = m_u64("correction_count").unwrap_or(0);
    let outcome = m_str_owned("task_outcome").unwrap_or_else(|| "unknown".to_string());
    let token_cost = m_u64("token_cost");
    let response_time_ms = m_u64("response_time_ms");
    let user_satisfaction = m_f64("user_satisfaction");

    // Move `parsed` here — context lives only in new-format body JSON
    let context = parsed.and_then(|b| b.get("context").cloned());

    ExtractedObs {
        obs_type,
        prompt_slug,
        score,
        sentiment,
        corrections,
        outcome,
        token_cost,
        response_time_ms,
        user_satisfaction,
        context,
    }
}

/// Aggregates built from a set of observation nodes.
struct PerfAggregates {
    total_count: usize,
    avg_score: f64,
    avg_sentiment: f64,
    avg_corrections: f64,
    avg_token_cost: Option<f64>,
    avg_response_time_ms: Option<f64>,
    task_outcomes: std::collections::HashMap<String, u64>,
}

/// Parse a context key=value filter from `?context=task_type:coding`.
/// Returns `(key, value)` or `None` if the param is absent or malformed.
fn parse_context_filter(s: Option<&str>) -> Option<(String, String)> {
    let s = s?;
    let mut parts = s.splitn(2, ':');
    let key = parts.next()?.trim().to_string();
    let val = parts.next()?.trim().to_string();
    if key.is_empty() || val.is_empty() {
        None
    } else {
        Some((key, val))
    }
}

/// Returns true if the observation node matches the context filter (key:value).
/// Checks body JSON `context.<key>`. Unreadable body or missing key = no match.
fn matches_context_filter(obs: &Node, key: &str, value: &str) -> bool {
    let Ok(body) = serde_json::from_str::<serde_json::Value>(&obs.data.body) else {
        return false;
    };
    body.get("context")
        .and_then(|c| c.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s == value)
        .unwrap_or(false)
}

fn aggregate_observations(obs_list: &[Node]) -> PerfAggregates {
    let total_count = obs_list.len();
    let mut sum_score = 0.0f64;
    let mut sum_sentiment = 0.0f64;
    let mut sum_corrections = 0u64;
    let mut sum_token_cost = 0u64;
    let mut token_cost_count = 0usize;
    let mut sum_response_time = 0u64;
    let mut response_time_count = 0usize;
    // Pre-allocate for the four known outcomes (success/partial/failure/unknown)
    let mut task_outcomes: std::collections::HashMap<String, u64> =
        std::collections::HashMap::with_capacity(4);

    for n in obs_list {
        let ex = extract_obs(n);
        sum_score += ex.score;
        sum_sentiment += ex.sentiment;
        sum_corrections += ex.corrections;
        *task_outcomes.entry(ex.outcome).or_insert(0) += 1;
        if let Some(tc) = ex.token_cost {
            sum_token_cost += tc;
            token_cost_count += 1;
        }
        if let Some(rt) = ex.response_time_ms {
            sum_response_time += rt;
            response_time_count += 1;
        }
    }

    let n = total_count as f64;
    PerfAggregates {
        total_count,
        avg_score: if total_count > 0 { sum_score / n } else { 0.0 },
        avg_sentiment: if total_count > 0 { sum_sentiment / n } else { 0.0 },
        avg_corrections: if total_count > 0 { sum_corrections as f64 / n } else { 0.0 },
        avg_token_cost: if token_cost_count > 0 {
            Some(sum_token_cost as f64 / token_cost_count as f64)
        } else {
            None
        },
        avg_response_time_ms: if response_time_count > 0 {
            Some(sum_response_time as f64 / response_time_count as f64)
        } else {
            None
        },
        task_outcomes,
    }
}

fn build_obs_detail(n: &Node) -> serde_json::Value {
    // Single parse via extract_obs — no double-parse for metrics vs context
    let ex = extract_obs(n);
    serde_json::json!({
        "id": n.id.to_string(),
        "observation_type": ex.obs_type,
        "prompt_slug": ex.prompt_slug,
        "score": ex.score,
        "observation_score": ex.score,
        "sentiment_score": ex.sentiment,
        "correction_count": ex.corrections,
        "task_outcome": ex.outcome,
        "token_cost": ex.token_cost,
        "response_time_ms": ex.response_time_ms,
        "user_satisfaction": ex.user_satisfaction,
        "context": ex.context,
        "created_at": n.created_at.to_rfc3339(),
    })
}

// ── GET /prompts/:slug/performance ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct PerformanceQuery {
    #[serde(default = "default_perf_limit")]
    limit: usize,
    /// Optional context filter: `key:value` (e.g. `task_type:coding`)
    context: Option<String>,
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

    let context_filter = parse_context_filter(q.context.as_deref());

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
        .filter(|n| {
            if let Some((ref key, ref val)) = context_filter {
                matches_context_filter(n, key, val)
            } else {
                true
            }
        })
        .collect();

    all_obs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let agg = aggregate_observations(&all_obs);

    all_obs.truncate(q.limit);
    let observations: Vec<serde_json::Value> = all_obs.iter().map(build_obs_detail).collect();

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "slug": slug,
        "prompt_id": prompt.id.to_string(),
        "context_filter": q.context,
        "observation_count": agg.total_count,
        "avg_score": agg.avg_score,
        "avg_sentiment": agg.avg_sentiment,
        "avg_correction_count": agg.avg_corrections,
        "avg_token_cost": agg.avg_token_cost,
        "avg_response_time_ms": agg.avg_response_time_ms,
        "task_outcomes": agg.task_outcomes,
        "observations_shown": observations.len(),
        "observations": observations,
    }))))
}

// ── GET /prompts/:slug/versions/:version/performance ─────────────────────────

#[derive(Deserialize)]
pub struct VersionPerfQuery {
    #[serde(default = "default_perf_limit")]
    limit: usize,
    /// Branch to look up the version on (defaults to "main")
    branch: Option<String>,
    /// Optional context filter: `key:value` (e.g. `task_type:coding`)
    context: Option<String>,
}

pub async fn version_performance(
    State(state): State<AppState>,
    Path((slug, version)): Path<(String, u32)>,
    Query(q): Query<VersionPerfQuery>,
) -> AppResult<impl IntoResponse> {
    let branch = q.branch.as_deref().unwrap_or("main");
    let resolver = PromptResolver::new(state.storage.clone());

    let version_node = resolver
        .get_version(&slug, branch, version)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}@{}/v{}' not found", slug, branch, version))?;

    let context_filter = parse_context_filter(q.context.as_deref());

    // Collect all performance observations linked via obs --[informed_by]--> this version node
    let informed_rel = rels::informed_by();
    let mut all_obs: Vec<Node> = state
        .storage
        .edges_to(version_node.id)?
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
        .filter(|n| {
            if let Some((ref key, ref val)) = context_filter {
                matches_context_filter(n, key, val)
            } else {
                true
            }
        })
        .collect();

    all_obs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let agg = aggregate_observations(&all_obs);

    all_obs.truncate(q.limit);
    let observations: Vec<serde_json::Value> = all_obs.iter().map(build_obs_detail).collect();

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "slug": slug,
        "version": version,
        "branch": branch,
        "version_node_id": version_node.id.to_string(),
        "context_filter": q.context,
        "observation_count": agg.total_count,
        "avg_score": agg.avg_score,
        "avg_sentiment": agg.avg_sentiment,
        "avg_correction_count": agg.avg_corrections,
        "avg_token_cost": agg.avg_token_cost,
        "avg_response_time_ms": agg.avg_response_time_ms,
        "task_outcomes": agg.task_outcomes,
        "observations_shown": observations.len(),
        "observations": observations,
    }))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::{kinds::defaults as kinds, Source};

    fn make_obs(body: &str) -> Node {
        Node::new(
            kinds::observation(),
            "test observation".to_string(),
            body.to_string(),
            Source { agent: "test".to_string(), session: None, channel: None },
            1.0,
        )
    }

    fn make_obs_with_meta(body: &str, meta: &[(&str, serde_json::Value)]) -> Node {
        let mut n = make_obs(body);
        for (k, v) in meta {
            n.data.metadata.insert(k.to_string(), v.clone());
        }
        n
    }

    // ── parse_context_filter ────────────────────────────────────────────────

    #[test]
    fn parse_context_filter_none_on_empty() {
        assert!(parse_context_filter(None).is_none());
        assert!(parse_context_filter(Some("")).is_none());
    }

    #[test]
    fn parse_context_filter_valid() {
        let f = parse_context_filter(Some("task_type:coding"));
        assert_eq!(f, Some(("task_type".to_string(), "coding".to_string())));
    }

    #[test]
    fn parse_context_filter_no_colon() {
        assert!(parse_context_filter(Some("nocolon")).is_none());
    }

    #[test]
    fn parse_context_filter_extra_colons() {
        // splits on first colon only
        let f = parse_context_filter(Some("task_type:a:b"));
        assert_eq!(f, Some(("task_type".to_string(), "a:b".to_string())));
    }

    #[test]
    fn parse_context_filter_empty_key() {
        assert!(parse_context_filter(Some(":value")).is_none());
    }

    #[test]
    fn parse_context_filter_empty_value() {
        assert!(parse_context_filter(Some("key:")).is_none());
    }

    // ── matches_context_filter ──────────────────────────────────────────────

    #[test]
    fn matches_context_filter_hit() {
        let body = r#"{"context":{"task_type":"coding"}}"#;
        let n = make_obs(body);
        assert!(matches_context_filter(&n, "task_type", "coding"));
    }

    #[test]
    fn matches_context_filter_miss_value() {
        let body = r#"{"context":{"task_type":"writing"}}"#;
        let n = make_obs(body);
        assert!(!matches_context_filter(&n, "task_type", "coding"));
    }

    #[test]
    fn matches_context_filter_miss_key() {
        let body = r#"{"context":{"topic":"rust"}}"#;
        let n = make_obs(body);
        assert!(!matches_context_filter(&n, "task_type", "coding"));
    }

    #[test]
    fn matches_context_filter_no_context_field() {
        let n = make_obs(r#"{"observation_type":"performance"}"#);
        assert!(!matches_context_filter(&n, "task_type", "coding"));
    }

    #[test]
    fn matches_context_filter_invalid_json() {
        let n = make_obs("not json");
        assert!(!matches_context_filter(&n, "task_type", "coding"));
    }

    // ── extract_obs ─────────────────────────────────────────────────────────

    #[test]
    fn extract_obs_full_body() {
        let body = serde_json::json!({
            "observation_type": "performance",
            "prompt_slug": "my-prompt",
            "metrics": {
                "observation_score": 0.85,
                "sentiment_score": 0.7,
                "correction_count": 2,
                "task_outcome": "success",
                "token_cost": 120,
                "response_time_ms": 350,
                "user_satisfaction": 0.9,
            },
            "context": { "task_type": "coding" }
        });
        let n = make_obs(&body.to_string());
        let ex = extract_obs(&n);
        assert_eq!(ex.obs_type, "performance");
        assert_eq!(ex.prompt_slug, Some("my-prompt".to_string()));
        assert!((ex.score - 0.85).abs() < 1e-9);
        assert!((ex.sentiment - 0.7).abs() < 1e-9);
        assert_eq!(ex.corrections, 2);
        assert_eq!(ex.outcome, "success");
        assert_eq!(ex.token_cost, Some(120));
        assert_eq!(ex.response_time_ms, Some(350));
        assert_eq!(ex.user_satisfaction, Some(0.9));
        assert!(ex.context.is_some());
    }

    #[test]
    fn extract_obs_missing_fields_use_defaults() {
        let n = make_obs(r#"{"observation_type":"performance"}"#);
        let ex = extract_obs(&n);
        assert_eq!(ex.obs_type, "performance");
        assert!(ex.prompt_slug.is_none());
        assert!((ex.score - 0.0).abs() < 1e-9);
        assert_eq!(ex.corrections, 0);
        assert_eq!(ex.outcome, "unknown");
        assert!(ex.token_cost.is_none());
        assert!(ex.context.is_none());
    }

    #[test]
    fn extract_obs_invalid_body_returns_defaults() {
        let n = make_obs("not valid json");
        let ex = extract_obs(&n);
        assert_eq!(ex.obs_type, "performance");
        assert!(ex.prompt_slug.is_none());
        assert!((ex.score - 0.0).abs() < 1e-9);
    }

    // ── aggregate_observations ──────────────────────────────────────────────

    #[test]
    fn aggregate_observations_empty() {
        let agg = aggregate_observations(&[]);
        assert_eq!(agg.total_count, 0);
        assert!((agg.avg_score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_observations_single() {
        let body = serde_json::json!({
            "observation_type": "performance",
            "metrics": {
                "observation_score": 0.8,
                "sentiment_score": 0.6,
                "correction_count": 1,
                "task_outcome": "success",
                "token_cost": 100,
                "response_time_ms": 200,
                "user_satisfaction": 0.9,
            }
        });
        let n = make_obs(&body.to_string());
        let agg = aggregate_observations(&[n]);
        assert_eq!(agg.total_count, 1);
        assert!((agg.avg_score - 0.8).abs() < 1e-9);
        assert!((agg.avg_sentiment - 0.6).abs() < 1e-9);
        assert!((agg.avg_corrections - 1.0).abs() < 1e-9);
        assert_eq!(agg.avg_token_cost, Some(100.0));
        assert_eq!(agg.avg_response_time_ms, Some(200.0));
        assert_eq!(agg.task_outcomes.get("success"), Some(&1u64));
    }

    #[test]
    fn aggregate_observations_averages_scores() {
        let mk = |score: f64| {
            let body = serde_json::json!({
                "observation_type": "performance",
                "metrics": { "observation_score": score, "task_outcome": "success" }
            });
            make_obs(&body.to_string())
        };
        let nodes = vec![mk(0.4), mk(0.6), mk(0.8)];
        let agg = aggregate_observations(&nodes);
        assert_eq!(agg.total_count, 3);
        assert!((agg.avg_score - (0.4 + 0.6 + 0.8) / 3.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_observations_token_cost_partial() {
        // only nodes with token_cost contribute to avg
        let b1 = serde_json::json!({
            "observation_type": "performance",
            "metrics": { "observation_score": 0.5, "token_cost": 100, "task_outcome": "success" }
        });
        let b2 = serde_json::json!({
            "observation_type": "performance",
            "metrics": { "observation_score": 0.5, "task_outcome": "success" }
        });
        let nodes = vec![make_obs(&b1.to_string()), make_obs(&b2.to_string())];
        let agg = aggregate_observations(&nodes);
        assert_eq!(agg.avg_token_cost, Some(100.0));
    }

    #[test]
    fn aggregate_observations_task_outcomes_counted() {
        let mk_outcome = |o: &str| {
            let body = serde_json::json!({
                "observation_type": "performance",
                "metrics": { "observation_score": 0.5, "task_outcome": o }
            });
            make_obs(&body.to_string())
        };
        let nodes = vec![
            mk_outcome("success"),
            mk_outcome("success"),
            mk_outcome("failure"),
        ];
        let agg = aggregate_observations(&nodes);
        assert_eq!(agg.task_outcomes.get("success"), Some(&2u64));
        assert_eq!(agg.task_outcomes.get("failure"), Some(&1u64));
    }

    // ── build_obs_detail ────────────────────────────────────────────────────

    #[test]
    fn build_obs_detail_includes_all_fields() {
        let body = serde_json::json!({
            "observation_type": "performance",
            "prompt_slug": "slug-a",
            "metrics": {
                "observation_score": 0.9,
                "sentiment_score": 0.8,
                "correction_count": 0,
                "task_outcome": "success",
                "token_cost": 50,
                "response_time_ms": 100,
                "user_satisfaction": 1.0,
            },
            "context": { "task_type": "coding" }
        });
        let n = make_obs(&body.to_string());
        let v = build_obs_detail(&n);
        // build_obs_detail outputs: id, observation_score, sentiment_score,
        // correction_count, task_outcome, token_cost, response_time_ms,
        // user_satisfaction, context, created_at
        assert_eq!(v["task_outcome"], "success");
        assert!((v["observation_score"].as_f64().unwrap() - 0.9).abs() < 1e-9);
        assert!((v["sentiment_score"].as_f64().unwrap() - 0.8).abs() < 1e-9);
        assert_eq!(v["token_cost"], 50);
        assert_eq!(v["response_time_ms"], 100);
        assert!((v["user_satisfaction"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(v["context"]["task_type"], "coding");
    }

    #[test]
    fn build_obs_detail_null_optional_fields() {
        let body = serde_json::json!({
            "observation_type": "performance",
            "metrics": { "observation_score": 0.5, "task_outcome": "unknown" }
        });
        let n = make_obs(&body.to_string());
        let v = build_obs_detail(&n);
        assert!(v["token_cost"].is_null());
        assert!(v["response_time_ms"].is_null());
        assert!(v["user_satisfaction"].is_null());
        assert!(v["context"].is_null());
    }

    // ── ObsBodyJson serialisation ─────────────────────────────────────────────

    #[test]
    fn obs_body_json_roundtrip() {
        let orig = ObsBodyJson {
            agent: "agent-1".to_string(),
            prompt_slug: "slug-x".to_string(),
            prompt_version: Some(2),
            observation_type: "performance".to_string(),
            metrics: ObsMetrics {
                correction_count: 1,
                sentiment_score: 0.7,
                task_completed: true,
                task_outcome: "success".to_string(),
                observation_score: 0.85,
                token_cost: Some(99),
                response_time_ms: Some(300),
                user_satisfaction: Some(0.8),
            },
            context: ObsContext {
                task_type: "coding".to_string(),
                topic: Some("rust".to_string()),
                session_length: Some(5),
                message_count: Some(20),
                correction_rate: None,
                topic_shift: None,
                energy: None,
            },
        };
        let json = serde_json::to_string(&orig).unwrap();
        let decoded: ObsBodyJson = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.agent, "agent-1");
        assert_eq!(decoded.prompt_slug, "slug-x");
        assert_eq!(decoded.metrics.token_cost, Some(99u32));
        assert_eq!(decoded.context.topic, Some("rust".to_string()));
    }

    #[test]
    fn obs_body_json_defaults_for_missing_fields() {
        // prompt_slug defaults to empty string; prompt_version and optional metrics absent
        let json = r#"{"agent":"a","prompt_slug":"","observation_type":"performance","metrics":{"correction_count":0,"sentiment_score":0.5,"task_completed":false,"task_outcome":"unknown","observation_score":0.5},"context":{"task_type":"general"}}"#;
        let decoded: ObsBodyJson = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.prompt_slug, "");
        assert!(decoded.prompt_version.is_none());
        assert!(decoded.metrics.token_cost.is_none());
        assert!(decoded.context.topic.is_none());
    }

    // ── Input validation ──────────────────────────────────────────────────────

    #[test]
    fn sentiment_clamp() {
        let clamped_low: f32 = (-0.5_f32).clamp(0.0, 1.0);
        let clamped_high: f32 = (1.5_f32).clamp(0.0, 1.0);
        assert!((clamped_low - 0.0).abs() < 1e-6);
        assert!((clamped_high - 1.0).abs() < 1e-6);
    }

    #[test]
    fn task_outcome_normalization() {
        let normalize = |s: &str| -> String {
            match s {
                "success" | "partial" | "failure" | "unknown" => s.to_string(),
                _ => "unknown".to_string(),
            }
        };
        assert_eq!(normalize("success"), "success");
        assert_eq!(normalize("partial"), "partial");
        assert_eq!(normalize("failure"), "failure");
        assert_eq!(normalize("unknown"), "unknown");
        assert_eq!(normalize("bogus"), "unknown");
        assert_eq!(normalize("SUCCESS"), "unknown"); // case-sensitive
        assert_eq!(normalize(""), "unknown");
    }
}
