//! The router over HTTP semantics, driven with `oneshot`: no network, no
//! wall clock.

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sim_api::app;
use tower::ServiceExt;

const ORIGIN: &str = "https://rails.example";

async fn send(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, body)
}

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap()
}

#[tokio::test]
async fn health_answers_ok() {
    let (status, _, body) = send(app(None), request(Method::GET, "/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn an_unknown_route_is_a_404_envelope() {
    let (status, _, body) = send(app(None), request(Method::GET, "/no-such-route")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn a_wrong_method_is_a_405_envelope() {
    let (status, _, body) = send(app(None), request(Method::POST, "/health")).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(error_code(&body), "method_not_allowed");
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

    let origin = HeaderValue::from_static(ORIGIN);
    let (_, headers, _) = send(app(Some(origin)), with_origin()).await;
    assert_eq!(
        headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static(ORIGIN))
    );

    let (_, headers, _) = send(app(None), with_origin()).await;
    assert_eq!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN), None);
}

#[tokio::test]
async fn a_cors_preflight_for_a_post_is_answered() {
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/health")
        .header(header::ORIGIN, ORIGIN)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = send(app(Some(HeaderValue::from_static(ORIGIN))), preflight).await;
    assert!(status.is_success(), "{status}");
    let methods = headers
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(methods.contains("POST"), "{methods}");
}
