//! The router. Building it is pure, so tests drive it with `oneshot` and no
//! network.

use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use tower_http::cors::CorsLayer;

use crate::error::{ApiError, METHOD_NOT_ALLOWED, NOT_FOUND};

/// With an `allowed_origin`, the static frontend on that origin may call the
/// API (frontend ask #6). Without one, there's no CORS, which suits local
/// development, where the Vite proxy makes requests same-origin.
pub fn app(allowed_origin: Option<HeaderValue>) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed);
    match allowed_origin {
        Some(origin) => router.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE]),
        ),
        None => router,
    }
}

/// For Fly.io's health check (D1).
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, NOT_FOUND, "no such route")
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        METHOD_NOT_ALLOWED,
        "this route doesn't accept that method",
    )
}
