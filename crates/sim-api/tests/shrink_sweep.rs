//! `POST /shrink` and `POST /sweep` (specs/sim-api-plan.md, API5).

mod common;

use axum::http::StatusCode;
use common::{failed, post};
use serde_json::{Value, json};
use sim_api::encode::{MAX_PLAN_FAULTS, decode_run};
use sim_core::sweep::MAX_SWEEP_SEEDS;
use sim_scenarios::scenarios;

fn shrink_request(scenario_id: &str, handler: &str, plan: Value, invariant: &str) -> Value {
    json!({ "scenario_id": scenario_id, "seed": 0, "handler": handler, "fault_plan": plan, "invariant": invariant })
}

fn sweep_request(scenario_id: &str, seed_start: Value, count: Value) -> Value {
    json!({ "scenario_id": scenario_id, "seed_start": seed_start, "count": count })
}

#[tokio::test]
async fn shrink_reduces_a_generated_plan_on_its_failing_invariant() {
    // charge-retry's seed-0 plan has 3 faults and breaks naive.
    let run = post(
        "/run",
        &json!({ "scenario_id": "charge-retry", "seed": 0, "handler": "naive", "fault_plan": null }),
    )
    .await
    .json();
    let invariant = failed(&run)[0].clone();

    let reply = post(
        "/shrink",
        &shrink_request("charge-retry", "naive", Value::Null, &invariant),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["original"], run["fault_plan"]);
    assert_eq!(body["original"].as_array().unwrap().len(), 3);
    assert_eq!(body["shrunk"].as_array().unwrap().len(), 1);
    assert_eq!(body["invariant"], invariant);
    assert_eq!(body["candidates_tried"], 3);

    let shrunk_run = &body["run"];
    assert!(failed(shrunk_run).contains(&invariant));
    assert_eq!(shrunk_run["fault_plan"], body["shrunk"]);
    let link = decode_run(shrunk_run["replay"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(link.fault_plan).unwrap(),
        body["shrunk"]
    );
}

#[tokio::test]
async fn shrink_rejects_what_it_cant_shrink() {
    let story =
        serde_json::to_value(sim_scenarios::find("charge-retry").unwrap().story_plan).unwrap();
    let over_cap: Vec<Value> = (0..=MAX_PLAN_FAULTS)
        .map(|at| json!({ "CrashRestart": { "at": at } }))
        .collect();
    let cases = [
        // Hardened passes the story, so there's no failure to keep.
        (
            shrink_request(
                "charge-retry",
                "hardened",
                story.clone(),
                "single_capture_per_intent",
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "does_not_fail",
        ),
        (
            shrink_request("charge-retry", "naive", story.clone(), "no_such_invariant"),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_invariant",
        ),
        (
            shrink_request(
                "charge-retry",
                "naive",
                json!(over_cap),
                "single_capture_per_intent",
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "plan_too_long",
        ),
        (
            shrink_request(
                "no-such-scenario",
                "naive",
                story,
                "single_capture_per_intent",
            ),
            StatusCode::NOT_FOUND,
            "unknown_scenario",
        ),
    ];
    for (request, status, code) in cases {
        let reply = post("/shrink", &request).await;
        assert_eq!(reply.status, status, "{code}");
        assert_eq!(reply.error_code(), code);
    }
}

#[tokio::test]
async fn sweep_reports_hardened_never_failing() {
    for scenario in scenarios() {
        let reply = post("/sweep", &sweep_request(scenario.id, json!(0), json!(100))).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", scenario.id);
        let body = reply.json();
        assert_eq!(body["count"], 100, "{}", scenario.id);
        assert_eq!(body["hardened"]["failed"], 0, "{}", scenario.id);
        assert!(
            body["naive"]["failed"].as_u64().unwrap() > 0,
            "{}",
            scenario.id
        );
    }
}

#[tokio::test]
async fn sweep_rejects_bad_ranges() {
    let cases = [
        (
            sweep_request("charge-retry", json!(0), json!(MAX_SWEEP_SEEDS + 1)),
            StatusCode::UNPROCESSABLE_ENTITY,
            "too_many_seeds",
        ),
        (
            sweep_request("charge-retry", json!(u32::MAX), json!(2)),
            StatusCode::UNPROCESSABLE_ENTITY,
            "seed_overflow",
        ),
        (
            sweep_request("charge-retry", json!(0), json!(-1)),
            StatusCode::BAD_REQUEST,
            "bad_request",
        ),
        (
            sweep_request("no-such-scenario", json!(0), json!(10)),
            StatusCode::NOT_FOUND,
            "unknown_scenario",
        ),
    ];
    for (request, status, code) in cases {
        let reply = post("/sweep", &request).await;
        assert_eq!(reply.status, status, "{code}");
        assert_eq!(reply.error_code(), code);
    }
}
