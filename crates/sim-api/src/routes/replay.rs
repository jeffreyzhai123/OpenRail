//! `GET /replay/{encoded}`: a share link's run, recomputed.

use axum::Json;
use axum::extract::Path;
use axum::extract::rejection::PathRejection;

use super::compute;
use super::run::execute;
use crate::dto::RunResponse;
use crate::encode::decode_run;
use crate::error::ApiError;

/// Answers exactly as `POST /run` does for the decoded request, so a link
/// taken from a response replays to the same bytes and the same trace hash,
/// which is what the UI's "verified identical" badge relies on.
pub(crate) async fn get_replay(
    encoded: Result<Path<String>, PathRejection>,
) -> Result<Json<RunResponse>, ApiError> {
    let Path(encoded) = encoded.map_err(|_| ApiError::invalid_replay())?;
    let replay = decode_run(&encoded)?;
    let response = compute(move || {
        execute(
            replay.scenario_id,
            replay.seed,
            replay.handler,
            Some(replay.fault_plan),
        )
    })
    .await?;
    Ok(Json(response))
}
