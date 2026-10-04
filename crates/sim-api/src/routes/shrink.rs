//! `POST /shrink`.

use axum::Json;
use sim_core::fault::generate_fault_plan;
use sim_core::shrink::shrink_run;

use super::{ApiJson, check_plan_length, compute, find_scenario};
use crate::dto::{RunResponse, ShrinkRequest, ShrinkResponse};
use crate::error::ApiError;

pub(crate) async fn post_shrink(
    ApiJson(request): ApiJson<ShrinkRequest>,
) -> Result<Json<ShrinkResponse>, ApiError> {
    let response = compute(move || shrink(request)).await?;
    Ok(Json(response))
}

fn shrink(request: ShrinkRequest) -> Result<ShrinkResponse, ApiError> {
    let scenario = find_scenario(&request.scenario_id)?;
    let plan = request
        .fault_plan
        .unwrap_or_else(|| generate_fault_plan(request.seed, &scenario.workload));
    check_plan_length(&plan)?;

    let handler = request.handler;
    let result = shrink_run(
        &scenario.initial_ledger,
        &scenario.workload,
        request.seed,
        &plan,
        &|| handler.build(),
        &request.invariant,
    )?;
    let run = RunResponse::new(request.scenario_id, request.seed, handler, result.run)?;
    Ok(ShrinkResponse {
        original: result.original,
        shrunk: result.shrunk,
        invariant: result.invariant,
        candidates_tried: result.candidates_tried,
        run,
    })
}
