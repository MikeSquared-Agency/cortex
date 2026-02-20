use super::{AppResult, AppState, JsonResponse};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    Json as AxumJson,
};
use cortex_core::prompt::{PromptContent, PromptResolver};
use cortex_core::Storage;
use serde::Deserialize;

fn not_found(msg: impl Into<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(JsonResponse::<()>::err(msg.into())),
    )
        .into_response()
}

fn bad_request(msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(JsonResponse::<()>::err(msg.into())),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct BranchQuery {
    pub branch: Option<String>,
}

#[derive(Deserialize)]
pub struct CreatePromptBody {
    pub slug: String,
    #[serde(rename = "type")]
    pub prompt_type: String,
    pub branch: Option<String>,
    pub sections: std::collections::HashMap<String, serde_json::Value>,
    pub metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub override_sections: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct CreateVersionBody {
    #[serde(rename = "type")]
    pub prompt_type: Option<String>,
    pub branch: Option<String>,
    pub sections: std::collections::HashMap<String, serde_json::Value>,
    pub metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub override_sections: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub author: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateBranchBody {
    pub new_branch: String,
    pub from_branch: Option<String>,
    pub base_version: Option<u32>,
    pub author: Option<String>,
}

/// GET /prompts — list the HEAD of every slug+branch
pub async fn list_prompts(State(state): State<AppState>) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let prompts = resolver.list_all_prompts()?;
    Ok(Json(JsonResponse::ok(prompts)).into_response())
}

/// POST /prompts — create the first version of a prompt
pub async fn create_prompt(
    State(state): State<AppState>,
    AxumJson(body): AxumJson<CreatePromptBody>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let branch = body.branch.as_deref().unwrap_or("main").to_string();
    let author = body.author.as_deref().unwrap_or("http").to_string();

    let content = PromptContent {
        slug: body.slug.clone(),
        prompt_type: body.prompt_type,
        branch: branch.clone(),
        version: 1,
        sections: body.sections,
        metadata: body.metadata.unwrap_or_default(),
        override_sections: body.override_sections.unwrap_or_default(),
    };

    match resolver.create_prompt(content, &branch, &author) {
        Ok(node_id) => Ok(Json(JsonResponse::ok(serde_json::json!({
            "node_id": node_id.to_string(),
            "slug": body.slug,
            "version": 1,
            "branch": branch,
        })))
        .into_response()),
        Err(cortex_core::CortexError::Validation(msg)) if msg.contains("already exists") => {
            Ok(bad_request(msg))
        }
        Err(e) => Err(e.into()),
    }
}

/// GET /prompts/:slug/latest?branch=main — resolve HEAD with inheritance
pub async fn get_latest(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<BranchQuery>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let branch = query.branch.as_deref().unwrap_or("main");

    match resolver.find_head(&slug, branch)? {
        None => Ok(not_found(format!("Prompt '{}@{}' not found", slug, branch))),
        Some(node) => {
            let resolved = resolver.resolve(&node)?;
            Ok(Json(JsonResponse::ok(resolved)).into_response())
        }
    }
}

/// GET /prompts/:slug/versions?branch=main — version history
pub async fn list_versions(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<BranchQuery>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let branch = query.branch.as_deref().unwrap_or("main");

    let versions = resolver.list_versions(&slug, branch)?;
    if versions.is_empty() {
        return Ok(not_found(format!("Prompt '{}@{}' not found", slug, branch)));
    }

    Ok(Json(JsonResponse::ok(versions)).into_response())
}

/// GET /prompts/:slug/versions/:version?branch=main — raw specific version
pub async fn get_version(
    State(state): State<AppState>,
    Path((slug, version)): Path<(String, u32)>,
    Query(query): Query<BranchQuery>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let branch = query.branch.as_deref().unwrap_or("main");

    match resolver.get_version(&slug, branch, version)? {
        None => Ok(not_found(format!(
            "Prompt '{}@{}/v{}' not found",
            slug, branch, version
        ))),
        Some(node) => match resolver.parse_content(&node) {
            Err(e) => Ok(bad_request(e.to_string())),
            Ok(content) => {
                Ok(Json(JsonResponse::ok(serde_json::json!({
                    "slug": content.slug,
                    "type": content.prompt_type,
                    "version": content.version,
                    "branch": content.branch,
                    "raw_content": content,
                    "node_id": node.id.to_string(),
                    "created_at": node.created_at.to_rfc3339(),
                })))
                .into_response())
            }
        },
    }
}

/// POST /prompts/:slug/versions — create a new version
pub async fn create_version(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    AxumJson(body): AxumJson<CreateVersionBody>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let branch = body.branch.as_deref().unwrap_or("main").to_string();
    let author = body.author.as_deref().unwrap_or("http").to_string();

    // Determine prompt_type from body or inherit from existing HEAD.
    let prompt_type = if let Some(pt) = body.prompt_type {
        pt
    } else {
        match resolver.find_head(&slug, &branch)? {
            None => return Ok(not_found(format!("Prompt '{}@{}' not found", slug, branch))),
            Some(node) => resolver
                .parse_content(&node)
                .map(|c| c.prompt_type)
                .unwrap_or_else(|_| "unknown".to_string()),
        }
    };

    let content = PromptContent {
        slug: slug.clone(),
        prompt_type,
        branch: branch.clone(),
        version: 1, // overridden by create_version
        sections: body.sections,
        metadata: body.metadata.unwrap_or_default(),
        override_sections: body.override_sections.unwrap_or_default(),
    };

    match resolver.create_version(&slug, &branch, content, &author) {
        Ok(node_id) => {
            let new_node = state.storage.get_node(node_id)?.unwrap();
            let version = resolver
                .parse_content(&new_node)
                .map(|c| c.version)
                .unwrap_or(1);
            Ok(Json(JsonResponse::ok(serde_json::json!({
                "node_id": node_id.to_string(),
                "slug": slug,
                "version": version,
                "branch": branch,
            })))
            .into_response())
        }
        Err(cortex_core::CortexError::Validation(msg)) => Ok(not_found(msg)),
        Err(e) => Err(e.into()),
    }
}

/// POST /prompts/:slug/branch — fork onto a new branch
pub async fn create_branch(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    AxumJson(body): AxumJson<CreateBranchBody>,
) -> AppResult<Response> {
    let resolver = PromptResolver::new(state.storage.clone());
    let from_branch = body.from_branch.as_deref().unwrap_or("main");
    let author = body.author.as_deref().unwrap_or("http").to_string();

    match resolver.create_branch(&slug, from_branch, &body.new_branch, body.base_version, &author) {
        Ok(node_id) => Ok(Json(JsonResponse::ok(serde_json::json!({
            "node_id": node_id.to_string(),
            "slug": slug,
            "branch": body.new_branch,
            "from_branch": from_branch,
            "version": 1,
        })))
        .into_response()),
        Err(cortex_core::CortexError::Validation(msg)) => Ok(not_found(msg)),
        Err(e) => Err(e.into()),
    }
}
