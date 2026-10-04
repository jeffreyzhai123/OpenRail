//! The router over HTTP semantics, driven with `oneshot`: no network, no
//! wall clock.

mod common;

use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use common::{get, post_raw, send};
use serde_json::json;
use sim_api::app;

const ORIGIN: &str = "https://rails.example";

#[tokio::test]
async fn health_answers_ok() {
    let reply = get("/health").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json(), json!({ "status": "ok" }));
}

#[tokio::test]
async fn an_unknown_route_is_a_404_envelope() {
    let reply = get("/no-such-route").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.error_code(), "not_found");
    assert!(reply.json()["error"]["message"].is_string());
}

#[tokio::test]
async fn a_wrong_method_is_a_405_envelope() {
    let reply = post_raw("/health", None, Body::empty()).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.error_code(), "method_not_allowed");
}

#[tokio::test]
async fn cors_allows_the_configured_origin_only() {
    let with_origin = || {
        Request::builder()
            .uri("/health")
            .header(header::ORIGIN, ORIGIN)
            .body(Body::empty())
            .unwrap()
    };

    let reply = send(app(Some(HeaderValue::from_static(ORIGIN))), with_origin()).await;
    assert_eq!(
        reply.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static(ORIGIN))
    );

    let reply = send(app(None), with_origin()).await;
    assert_eq!(reply.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN), None);
}

#[tokio::test]
async fn a_cors_preflight_for_a_post_is_answered() {
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/run")
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let reply = send(app(Some(HeaderValue::from_static(ORIGIN))), preflight).await;
    assert!(reply.status.is_success(), "{}", reply.status);
    let methods = reply
        .headers
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(methods.contains("POST"), "{methods}");
}
