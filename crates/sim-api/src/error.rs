//! The error envelope. Every error response is
//! `{ "error": { "code", "message" } }`, so the frontend never decodes plain
//! text (specs/sim-api-plan.md, "Errors").

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

// Part of the API contract: the frontend matches on these codes.
pub(crate) const NOT_FOUND: &str = "not_found";
pub(crate) const METHOD_NOT_ALLOWED: &str = "method_not_allowed";

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: Body<'a>,
}

#[derive(Serialize)]
struct Body<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let envelope = Envelope {
            error: Body {
                code: self.code,
                message: &self.message,
            },
        };
        (self.status, Json(envelope)).into_response()
    }
}
