//! The error envelope. Every error response is
//! `{ "error": { "code", "message" } }`, so the frontend never decodes plain
//! text (specs/sim-api-plan.md, "Errors").

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sim_core::simulator::SimError;

use crate::encode::MAX_PLAN_FAULTS;

// Part of the API contract: the frontend matches on these codes.
pub(crate) const BAD_REQUEST: &str = "bad_request";
pub(crate) const NOT_FOUND: &str = "not_found";
pub(crate) const METHOD_NOT_ALLOWED: &str = "method_not_allowed";
pub(crate) const PAYLOAD_TOO_LARGE: &str = "payload_too_large";
pub(crate) const UNKNOWN_SCENARIO: &str = "unknown_scenario";
pub(crate) const INVALID_FAULT_PLAN: &str = "invalid_fault_plan";
pub(crate) const PLAN_TOO_LONG: &str = "plan_too_long";
pub(crate) const TIMEOUT: &str = "timeout";
pub(crate) const INTERNAL: &str = "internal";

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

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, BAD_REQUEST, message)
    }

    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, PAYLOAD_TOO_LARGE, message)
    }

    pub(crate) fn unknown_scenario(id: &str) -> Self {
        ApiError::new(
            StatusCode::NOT_FOUND,
            UNKNOWN_SCENARIO,
            format!("no scenario has the id {id:?}"),
        )
    }

    pub(crate) fn plan_too_long(len: usize) -> Self {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            PLAN_TOO_LONG,
            format!("a plan of {len} faults is over the cap of {MAX_PLAN_FAULTS}"),
        )
    }

    pub(crate) fn timeout() -> Self {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            TIMEOUT,
            "the simulation took too long",
        )
    }

    /// A server-side bug, not a problem with the request.
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL, message)
    }
}

impl From<SimError> for ApiError {
    fn from(error: SimError) -> Self {
        match error {
            SimError::InvalidFaultPlan(fault) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                INVALID_FAULT_PLAN,
                fault.to_string(),
            ),
            // The rest mean a scenario or handler is broken.
            other => ApiError::internal(other.to_string()),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        ApiError::internal(format!("couldn't encode the response: {error}"))
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
    /// Also writes 5xx errors to stderr, which Fly captures: they're the only
    /// ones that mean the server needs fixing.
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            eprintln!("sim-api {}: {}: {}", self.status, self.code, self.message);
        }
        let envelope = Envelope {
            error: Body {
                code: self.code,
                message: &self.message,
            },
        };
        (self.status, Json(envelope)).into_response()
    }
}
