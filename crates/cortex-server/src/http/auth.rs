use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

use super::JsonResponse;

/// Bearer token auth middleware. Skips `/health`. Short-circuits if auth is disabled.
pub async fn check(req: Request, next: Next, auth_enabled: bool, token: Option<String>) -> Response {
    if !auth_enabled {
        return next.run(req).await;
    }

    if req.uri().path() == "/health" {
        return next.run(req).await;
    }

    let expected = match token.as_deref() {
        Some(t) => t,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(JsonResponse::<()>::err("Auth enabled but no token configured")),
            )
                .into_response();
        }
    };

    match req.headers().get("authorization") {
        Some(value) => {
            let value = value.to_str().unwrap_or("");
            if value.starts_with("Bearer ") && &value[7..] == expected {
                next.run(req).await
            } else {
                (StatusCode::UNAUTHORIZED, Json(JsonResponse::<()>::err("Invalid token")))
                    .into_response()
            }
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(JsonResponse::<()>::err("Missing Authorization header")),
        )
            .into_response(),
    }
}
