/// Automatic prompt rollback HTTP endpoints (issue #23).
///
/// Endpoints:
///   POST /prompts/:slug/deploy           — record deployment + snapshot baseline
///   GET  /prompts/:slug/rollback-status  — current status (cooldown, quarantine, active window)
///   POST /prompts/:slug/unquarantine     — manually lift quarantine
use super::{AppResult, AppState, JsonResponse};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Json},
};
use cortex_core::{
    kinds::defaults as kinds,
    prompt::{rollback::compute_baseline_stats, PromptResolver, RollbackMonitor},
    relations::defaults as rels,
    Storage,
};
use serde::{Deserialize, Serialize};

// ── POST /prompts/:slug/deploy ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DeployBody {
    /// Branch to deploy (default: main).
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Agent that is deploying the prompt.
    pub agent_name: String,
    /// How many recent observations to use for baseline sampling (default: 20).
    #[serde(default = "default_baseline_sample")]
    pub baseline_sample_size: usize,
}

fn default_branch() -> String {
    "main".to_string()
}
fn default_baseline_sample() -> usize {
    20
}

#[derive(Serialize)]
struct DeployResponse {
    deployment_node_id: String,
    slug: String,
    branch: String,
    version: u32,
    prompt_node_id: String,
    baseline_correction_rate: f32,
    baseline_sentiment: f32,
    baseline_sample_size: usize,
}

pub async fn deploy_prompt(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<DeployBody>,
) -> AppResult<impl IntoResponse> {
    let resolver = PromptResolver::new(state.storage.clone());

    let head = resolver
        .find_head(&slug, &body.branch)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}@{}' not found", slug, body.branch))?;

    let content = resolver.parse_content(&head)?;
    let version = content.version;
    let prompt_node_id = head.id;

    // Collect recent observations for this slug to build baseline.
    // obs --[informed_by]--> any version of this slug
    let all_versions = resolver.find_versions(&slug, Some(&body.branch))?;
    let informed_rel = rels::informed_by();

    let mut baseline_obs: Vec<(f32, f32)> = Vec::new();
    for version_node in &all_versions {
        let obs_nodes: Vec<cortex_core::Node> = state
            .storage
            .edges_to(version_node.id)?
            .into_iter()
            .filter(|e| e.relation == informed_rel)
            .filter_map(|e| state.storage.get_node(e.from).ok().flatten())
            .filter(|n| n.kind == kinds::observation())
            .filter(|n| {
                n.data
                    .metadata
                    .get("observation_type")
                    .and_then(|v: &serde_json::Value| v.as_str())
                    == Some("performance")
            })
            .collect();

        for obs in obs_nodes {
            let meta = &obs.data.metadata;
            let correction = meta
                .get("correction_count")
                .and_then(|v: &serde_json::Value| v.as_f64())
                .unwrap_or(0.0) as f32;
            let sentiment = meta
                .get("sentiment_score")
                .and_then(|v: &serde_json::Value| v.as_f64())
                .unwrap_or(0.5) as f32;
            // Normalise correction count → rate (treat 5+ corrections as rate 1.0).
            let correction_rate = (correction / 5.0).min(1.0);
            baseline_obs.push((correction_rate, sentiment));
        }
    }

    baseline_obs.truncate(body.baseline_sample_size);
    let sample_size = baseline_obs.len();

    let (baseline_correction, _, baseline_sentiment, _) =
        compute_baseline_stats(&baseline_obs);

    let monitor = RollbackMonitor::new(state.storage.clone(), state.rollback_config.clone());

    let deployment_node_id = monitor.record_deployment(
        &slug,
        &body.branch,
        version,
        prompt_node_id,
        &body.agent_name,
        baseline_obs,
    )?;

    Ok(Json(JsonResponse::ok(DeployResponse {
        deployment_node_id: deployment_node_id.to_string(),
        slug,
        branch: body.branch,
        version,
        prompt_node_id: prompt_node_id.to_string(),
        baseline_correction_rate: baseline_correction,
        baseline_sentiment,
        baseline_sample_size: sample_size,
    })))
}

// ── GET /prompts/:slug/rollback-status ────────────────────────────────────────

#[derive(Deserialize)]
pub struct RollbackStatusQuery {
    #[serde(default = "default_branch")]
    pub branch: String,
}

pub async fn rollback_status(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<RollbackStatusQuery>,
) -> AppResult<impl IntoResponse> {
    let monitor = RollbackMonitor::new(state.storage.clone(), state.rollback_config.clone());
    match monitor.get_status(&slug, &q.branch)? {
        Some(s) => Ok(Json(JsonResponse::ok(s))),
        None => Err(anyhow::anyhow!("Prompt '{}@{}' not found", slug, q.branch).into()),
    }
}

// ── POST /prompts/:slug/unquarantine ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct UnquarantineBody {
    #[serde(default = "default_branch")]
    pub branch: String,
}

pub async fn unquarantine_prompt(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<UnquarantineBody>,
) -> AppResult<impl IntoResponse> {
    let resolver = PromptResolver::new(state.storage.clone());

    let head = resolver
        .find_head(&slug, &body.branch)?
        .ok_or_else(|| anyhow::anyhow!("Prompt '{}@{}' not found", slug, body.branch))?;

    let monitor = RollbackMonitor::new(state.storage.clone(), state.rollback_config.clone());
    monitor.unquarantine(head.id)?;

    Ok(Json(JsonResponse::ok(serde_json::json!({
        "prompt_node_id": head.id.to_string(),
        "slug": slug,
        "branch": body.branch,
        "quarantined": false,
    }))))
}
