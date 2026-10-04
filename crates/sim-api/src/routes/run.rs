//! `POST /run`.

use axum::Json;
use sim_core::fault::FaultPlan;
use sim_core::handlers::HandlerKind;
use sim_core::simulator::run;

use super::{ApiJson, check_plan_length, compute, find_scenario};
use crate::dto::{RunRequest, RunResponse};
use crate::error::ApiError;

pub(crate) async fn post_run(
    ApiJson(request): ApiJson<RunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    let response = compute(move || {
        execute(
            request.scenario_id,
            request.seed,
            request.handler,
            request.fault_plan,
        )
    })
    .await?;
    Ok(Json(response))
}

/// The run behind both `POST /run` and `GET /replay`, so the same input
/// always gives the same response. `None` generates a plan from the seed.
pub(crate) fn execute(
    scenario_id: String,
    seed: u32,
    handler: HandlerKind,
    fault_plan: Option<FaultPlan>,
) -> Result<RunResponse, ApiError> {
    let scenario = find_scenario(&scenario_id)?;
    if let Some(plan) = &fault_plan {
        check_plan_length(plan)?;
    }
    let result = run(
        &scenario.initial_ledger,
        &scenario.workload,
        seed,
        fault_plan.as_ref(),
        &|| handler.build(),
    )?;
    Ok(RunResponse::new(scenario_id, seed, handler, result)?)
}
