//! `POST /sweep`.

use axum::Json;
use sim_core::sweep::sweep;

use super::{ApiJson, compute, find_scenario};
use crate::dto::{SweepRequest, SweepResponse};
use crate::error::ApiError;

pub(crate) async fn post_sweep(
    ApiJson(request): ApiJson<SweepRequest>,
) -> Result<Json<SweepResponse>, ApiError> {
    let response = compute(move || {
        let scenario = find_scenario(&request.scenario_id)?;
        let result = sweep(
            &scenario.initial_ledger,
            &scenario.workload,
            request.seed_start,
            request.count,
        )?;
        Ok(SweepResponse::from(result))
    })
    .await?;
    Ok(Json(response))
}
