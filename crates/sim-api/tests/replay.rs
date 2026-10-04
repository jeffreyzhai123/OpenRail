//! `GET /replay/{encoded}` (specs/sim-api-plan.md, API4).

mod common;

use axum::http::StatusCode;
use common::{get, post};
use serde_json::{Value, json};
use sim_api::encode::{MAX_PLAN_FAULTS, MAX_REPLAY_LEN, Replay, encode_run};
use sim_core::event::EventId;
use sim_core::fault::FaultOp;
use sim_core::handlers::HandlerKind;
use sim_scenarios::scenarios;

fn link(scenario_id: &str, fault_plan: Vec<FaultOp>) -> String {
    encode_run(&Replay {
        scenario_id: scenario_id.to_string(),
        seed: 0,
        handler: HandlerKind::Naive,
        fault_plan,
    })
    .unwrap()
}

#[tokio::test]
async fn a_runs_replay_link_reproduces_it_byte_for_byte() {
    for scenario in scenarios() {
        for handler in ["naive", "hardened"] {
            let request = json!({ "scenario_id": scenario.id, "seed": 5, "handler": handler, "fault_plan": null });
            let original = post("/run", &request).await;
            let replay_link = original.json()["replay"].as_str().unwrap().to_string();

            let replayed = get(&format!("/replay/{replay_link}")).await;
            assert_eq!(replayed.status, StatusCode::OK, "{} {handler}", scenario.id);
            assert_eq!(replayed.bytes, original.bytes, "{} {handler}", scenario.id);
        }
    }
}

#[tokio::test]
async fn a_newer_encoding_version_is_unsupported() {
    let future = link("charge-retry", vec![]).replacen("1.", "9.", 1);
    let reply = get(&format!("/replay/{future}")).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.error_code(), "unsupported_encoding_version");
}

#[tokio::test]
async fn a_malformed_link_is_an_invalid_replay() {
    for bad in ["garbage", "1.not*base64", "%FF"] {
        let reply = get(&format!("/replay/{bad}")).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(reply.error_code(), "invalid_replay", "{bad}");
    }
}

#[tokio::test]
async fn an_over_long_link_is_a_413() {
    let long = format!("1.{}", "A".repeat(MAX_REPLAY_LEN));
    let reply = get(&format!("/replay/{long}")).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(reply.error_code(), "payload_too_large");
}

#[tokio::test]
async fn a_well_formed_link_still_gets_the_runs_own_checks() {
    let cases: [(String, StatusCode, &str); 3] = [
        (
            link("no-such-scenario", vec![]),
            StatusCode::NOT_FOUND,
            "unknown_scenario",
        ),
        (
            link(
                "charge-retry",
                vec![FaultOp::Drop {
                    event_id: EventId(99),
                }],
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_fault_plan",
        ),
        (
            link(
                "charge-retry",
                (0..=MAX_PLAN_FAULTS as u64)
                    .map(|at| FaultOp::CrashRestart { at })
                    .collect(),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "plan_too_long",
        ),
    ];
    for (encoded, status, code) in cases {
        let reply = get(&format!("/replay/{encoded}")).await;
        assert_eq!(reply.status, status, "{code}");
        assert_eq!(reply.error_code(), code);
    }
}

#[tokio::test]
async fn a_replayed_story_keeps_its_failure() {
    let story = sim_scenarios::find("charge-retry").unwrap().story_plan;
    let reply = get(&format!("/replay/{}", link("charge-retry", story))).await;
    let body: Value = reply.json();
    assert!(common::failed(&body).contains(&"single_capture_per_intent".to_string()));
}
