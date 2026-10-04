//! `GET /scenarios` and `POST /run` (specs/sim-api-plan.md, API3).

mod common;

use axum::http::StatusCode;
use common::{failed, get, post, post_raw};
use serde_json::{Value, json};
use sim_api::MAX_BODY_BYTES;
use sim_api::encode::{MAX_PLAN_FAULTS, Replay, decode_run};
use sim_core::fault::generate_fault_plan;
use sim_core::handlers::HandlerKind;
use sim_scenarios::scenarios;

fn run_request(scenario_id: &str, seed: u32, handler: &str, fault_plan: Value) -> Value {
    json!({ "scenario_id": scenario_id, "seed": seed, "handler": handler, "fault_plan": fault_plan })
}

fn story_plan(scenario_id: &str) -> Value {
    serde_json::to_value(sim_scenarios::find(scenario_id).unwrap().story_plan).unwrap()
}

#[tokio::test]
async fn scenarios_are_listed_in_registry_order() {
    let reply = get("/scenarios").await;
    assert_eq!(reply.status, StatusCode::OK);
    let listed = reply.json();
    let listed = listed.as_array().unwrap();
    let registered = scenarios();
    assert_eq!(listed.len(), registered.len());
    for (summary, scenario) in listed.iter().zip(&registered) {
        let accounts: Vec<&str> = scenario
            .initial_ledger
            .iter()
            .map(|(account, _)| account.as_str())
            .collect();
        assert_eq!(summary["id"], scenario.id);
        assert_eq!(summary["name"], scenario.name);
        assert_eq!(summary["description"], scenario.description);
        assert_eq!(summary["accounts"], json!(accounts));
        assert_eq!(
            summary["workload"],
            serde_json::to_value(&scenario.workload).unwrap()
        );
        assert_eq!(summary["story_plan"], story_plan(scenario.id));
    }
}

#[tokio::test]
async fn story_plans_turn_naive_red_and_leave_hardened_green() {
    for scenario in scenarios() {
        let naive = post(
            "/run",
            &run_request(scenario.id, 0, "naive", story_plan(scenario.id)),
        )
        .await;
        assert_eq!(naive.status, StatusCode::OK, "{}", scenario.id);
        assert!(!failed(&naive.json()).is_empty(), "{}", scenario.id);

        let hardened = post(
            "/run",
            &run_request(scenario.id, 0, "hardened", story_plan(scenario.id)),
        )
        .await;
        assert_eq!(
            failed(&hardened.json()),
            Vec::<String>::new(),
            "{}",
            scenario.id
        );
    }
    // V1's acceptance check (v1-mvp-plan.md).
    let retry = post(
        "/run",
        &run_request("charge-retry", 0, "naive", story_plan("charge-retry")),
    )
    .await;
    assert!(failed(&retry.json()).contains(&"single_capture_per_intent".to_string()));
}

#[tokio::test]
async fn a_null_plan_comes_back_as_the_seeds_generated_plan() {
    let scenario = sim_scenarios::find("charge-retry").unwrap();
    let reply = post(
        "/run",
        &run_request("charge-retry", 7, "naive", Value::Null),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let generated = generate_fault_plan(7, &scenario.workload);
    assert_eq!(
        reply.json()["fault_plan"],
        serde_json::to_value(generated).unwrap()
    );
}

#[tokio::test]
async fn the_same_request_gives_byte_identical_responses() {
    let request = run_request("late-ach-return", 3, "hardened", Value::Null);
    let first = post("/run", &request).await;
    let second = post("/run", &request).await;
    assert_eq!(first.bytes, second.bytes);
}

#[tokio::test]
async fn the_replay_link_encodes_the_effective_plan() {
    let reply = post(
        "/run",
        &run_request("refund-before-capture", 11, "naive", Value::Null),
    )
    .await;
    let body = reply.json();
    let decoded = decode_run(body["replay"].as_str().unwrap()).unwrap();
    assert_eq!(
        decoded,
        Replay {
            scenario_id: "refund-before-capture".to_string(),
            seed: 11,
            handler: HandlerKind::Naive,
            fault_plan: serde_json::from_value(body["fault_plan"].clone()).unwrap(),
        }
    );
}

#[tokio::test]
async fn an_unknown_scenario_is_a_404() {
    let reply = post(
        "/run",
        &run_request("no-such-scenario", 0, "naive", Value::Null),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.error_code(), "unknown_scenario");
}

#[tokio::test]
async fn a_plan_naming_an_unknown_event_is_a_422() {
    let plan = json!([{ "Drop": { "event_id": 99 } }]);
    let reply = post("/run", &run_request("charge-retry", 0, "naive", plan)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.error_code(), "invalid_fault_plan");
}

#[tokio::test]
async fn a_plan_over_the_cap_is_a_422() {
    let plan: Vec<Value> = (0..=MAX_PLAN_FAULTS)
        .map(|at| json!({ "CrashRestart": { "at": at } }))
        .collect();
    let reply = post(
        "/run",
        &run_request("charge-retry", 0, "naive", json!(plan)),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.error_code(), "plan_too_long");
}

#[tokio::test]
async fn bad_requests_are_400_envelopes() {
    let bad_bodies = [
        json!({ "scenario_id": "charge-retry", "seed": 0, "handler": "bogus", "fault_plan": null }),
        json!({ "scenario_id": "charge-retry", "seed": -1, "handler": "naive", "fault_plan": null }),
        json!({ "scenario_id": "charge-retry", "seed": 4_294_967_296_u64, "handler": "naive", "fault_plan": null }),
        json!({ "scenario_id": "charge-retry", "seed": 0, "handler": "naive", "fault_plan": null, "extra": 1 }),
        json!({ "seed": 0, "handler": "naive" }),
    ];
    for body in bad_bodies {
        let reply = post("/run", &body).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(reply.error_code(), "bad_request", "{body}");
    }

    let malformed = post_raw("/run", Some("application/json"), "{ not json").await;
    assert_eq!(malformed.error_code(), "bad_request");
    let no_content_type = post_raw(
        "/run",
        None,
        run_request("charge-retry", 0, "naive", Value::Null).to_string(),
    )
    .await;
    assert_eq!(no_content_type.status, StatusCode::BAD_REQUEST);
    assert_eq!(no_content_type.error_code(), "bad_request");
}

#[tokio::test]
async fn an_oversized_body_is_a_413_envelope() {
    let padding = "x".repeat(MAX_BODY_BYTES);
    let body = json!({ "scenario_id": padding, "seed": 0, "handler": "naive", "fault_plan": null });
    let reply = post("/run", &body).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(reply.error_code(), "payload_too_large");
}
