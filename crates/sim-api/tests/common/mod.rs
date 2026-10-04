//! Helpers shared by the HTTP test files. Each test crate uses a different
//! subset, which is why dead code is allowed here.
#![allow(dead_code)]

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Bytes,
}

impl Reply {
    pub fn json(&self) -> Value {
        if self.bytes.is_empty() {
            return Value::Null;
        }
        serde_json::from_slice(&self.bytes).unwrap()
    }

    pub fn error_code(&self) -> String {
        self.json()["error"]["code"]
            .as_str()
            .unwrap_or_else(|| panic!("not an error envelope: {:?}", self.json()))
            .to_string()
    }
}

pub async fn send(router: Router, request: Request<Body>) -> Reply {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        bytes,
    }
}

pub fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub async fn get(uri: &str) -> Reply {
    send(sim_api::app(None), request(Method::GET, uri)).await
}

pub async fn post(uri: &str, body: &Value) -> Reply {
    post_raw(uri, Some("application/json"), body.to_string()).await
}

pub async fn post_raw(uri: &str, content_type: Option<&str>, body: impl Into<Body>) -> Reply {
    let mut builder = Request::builder().method(Method::POST).uri(uri);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    send(sim_api::app(None), builder.body(body.into()).unwrap()).await
}

/// The names of a run response's failed invariants, in check order.
pub fn failed(run: &Value) -> Vec<String> {
    run["invariants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|invariant| invariant["passed"] == Value::Bool(false))
        .map(|invariant| invariant["name"].as_str().unwrap().to_string())
        .collect()
}
