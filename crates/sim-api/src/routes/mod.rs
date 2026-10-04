//! Route handlers, plus what they share: the JSON extractor, the helper that
//! runs simulation work, and lookups.

pub(crate) mod replay;
pub(crate) mod run;
pub(crate) mod scenarios;

use std::time::Duration;

use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use sim_core::fault::FaultOp;
use sim_scenarios::Scenario;

use crate::encode::MAX_PLAN_FAULTS;
use crate::error::ApiError;

/// Simulation work over this answers `timeout`. The caps keep real requests
/// far below it.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// `Json`, except every rejection answers with the error envelope.
pub(crate) struct ApiJson<T>(pub(crate) T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Err(ApiError::payload_too_large(rejection.body_text()))
            }
            Err(rejection) => Err(ApiError::bad_request(rejection.body_text())),
        }
    }
}

/// Runs simulation work on a blocking thread, so it never stalls the async
/// workers, under `REQUEST_TIMEOUT`.
pub(crate) async fn compute<T>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError>
where
    T: Send + 'static,
{
    compute_within(REQUEST_TIMEOUT, work).await
}

/// Work that times out keeps running on its blocking thread until it ends,
/// but nothing waits for it; the caps bound how long that can be.
async fn compute_within<T>(
    limit: Duration,
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError>
where
    T: Send + 'static,
{
    match tokio::time::timeout(limit, tokio::task::spawn_blocking(work)).await {
        Err(_elapsed) => Err(ApiError::timeout()),
        Ok(Err(join_error)) => Err(ApiError::internal(format!(
            "the simulation failed: {join_error}"
        ))),
        Ok(Ok(outcome)) => outcome,
    }
}

pub(crate) fn find_scenario(id: &str) -> Result<Scenario, ApiError> {
    sim_scenarios::find(id).ok_or_else(|| ApiError::unknown_scenario(id))
}

pub(crate) fn check_plan_length(plan: &[FaultOp]) -> Result<(), ApiError> {
    if plan.len() > MAX_PLAN_FAULTS {
        return Err(ApiError::plan_too_long(plan.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use axum::response::IntoResponse;

    use super::*;

    #[tokio::test]
    async fn work_that_outlasts_the_limit_times_out() {
        // The work can't finish until `release` is dropped, so the timeout
        // always wins. Dropping it afterwards lets the blocked thread end.
        let (release, blocked) = mpsc::channel::<()>();
        let outcome = compute_within(Duration::from_millis(10), move || {
            let _ = blocked.recv();
            Ok(())
        })
        .await;
        drop(release);
        let status = outcome.unwrap_err().into_response().status();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn work_that_panics_is_an_internal_error() {
        let outcome =
            compute_within::<()>(Duration::from_secs(5), || panic!("broken handler")).await;
        let status = outcome.unwrap_err().into_response().status();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn work_that_finishes_returns_its_outcome() {
        let outcome = compute_within(Duration::from_secs(5), || Ok(7)).await;
        assert_eq!(outcome.unwrap(), 7);
    }
}
