pub mod prompts;
pub mod rollback;
mod routes;
pub mod selection;
mod viz;

pub use routes::create_router;
pub use viz::GRAPH_VIZ_HTML;

use cortex_core::{Node, NodeFilter, NodeKind, Storage};

/// Find a node by kind and title (linear scan — no title index in storage).
///
/// Returns the first node whose `data.title` exactly matches `title`, or `None`.
/// Shared by `routes` and `selection` to avoid duplicate implementations.
pub(super) fn find_by_title(
    storage: &cortex_core::RedbStorage,
    kind: &NodeKind,
    title: &str,
) -> cortex_core::Result<Option<Node>> {
    let nodes = storage.list_nodes(NodeFilter::new().with_kinds(vec![kind.clone()]))?;
    Ok(nodes.into_iter().find(|n| n.data.title == title))
}

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use cortex_core::briefing::BriefingEngine;
use cortex_core::prompt::RollbackConfig;
use cortex_core::{FastEmbedService, GraphEngineImpl, HnswIndex, RedbStorage, RwLockVectorIndex};
use serde::Serialize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

/// Concrete briefing engine type shared across HTTP handlers
pub type HttpBriefingEngine = BriefingEngine<
    RedbStorage,
    Arc<FastEmbedService>,
    RwLockVectorIndex<HnswIndex>,
    Arc<GraphEngineImpl<RedbStorage>>,
>;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<cortex_core::RedbStorage>,
    pub graph_engine: Arc<cortex_core::GraphEngineImpl<cortex_core::RedbStorage>>,
    pub vector_index: Arc<std::sync::RwLock<cortex_core::HnswIndex>>,
    pub embedding_service: Arc<cortex_core::FastEmbedService>,
    pub auto_linker: Arc<
        std::sync::RwLock<
            cortex_core::AutoLinker<
                cortex_core::RedbStorage,
                cortex_core::FastEmbedService,
                cortex_core::HnswIndex,
                cortex_core::GraphEngineImpl<cortex_core::RedbStorage>,
            >,
        >,
    >,
    pub graph_version: Arc<AtomicU64>,
    pub briefing_engine: Arc<HttpBriefingEngine>,
    pub start_time: std::time::Instant,
    pub rollback_config: RollbackConfig,
}

/// JSON response wrapper
#[derive(Serialize)]
pub struct JsonResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> JsonResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> JsonResponse<()> {
        JsonResponse {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// Custom error type for HTTP handlers
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(JsonResponse::<()>::err(self.0.to_string())),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;
