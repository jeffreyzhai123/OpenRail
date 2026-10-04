//! `GET /scenarios`.

use axum::Json;

use crate::dto::ScenarioSummary;

/// In registry order, which is the order the UI lists them.
pub(crate) async fn list_scenarios() -> Json<Vec<ScenarioSummary>> {
    Json(
        sim_scenarios::scenarios()
            .into_iter()
            .map(ScenarioSummary::from)
            .collect(),
    )
}
