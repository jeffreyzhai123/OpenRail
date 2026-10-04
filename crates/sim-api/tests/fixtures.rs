//! Golden fixtures for the frontend (frontend ask #7, decision G). The API's
//! canonical responses are checked into `frontend/src/api/fixtures/`, so the
//! UI is built against real serde output. By default this test only
//! compares; `UPDATE_FIXTURES=1` rewrites the files.

mod common;

use std::env;
use std::fs;
use std::path::PathBuf;

use axum::http::StatusCode;
use common::{Reply, failed, get, post};
use serde_json::{Value, json};
use sim_scenarios::scenarios;

/// The seed every fixture uses. Story plans are explicit, so it only matters
/// for the shrink fixture's generated plan.
const SEED: u32 = 0;
/// Enough seeds for a readable chart in the sweep fixture.
const SWEEP_SEEDS: u32 = 100;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/api/fixtures")
}

/// Pretty-printed with sorted keys, so diffs read well. The API's own byte
/// order is pinned by the byte-identical tests in run.rs and replay.rs.
fn render(body: &Value) -> String {
    let mut rendered = serde_json::to_string_pretty(body).unwrap();
    rendered.push('\n');
    rendered
}

fn ok(reply: Reply, what: &str) -> Value {
    assert_eq!(reply.status, StatusCode::OK, "{what}: {:?}", reply.json());
    reply.json()
}

async fn canonical_responses() -> Vec<(String, Value)> {
    let mut fixtures = vec![(
        "scenarios.json".to_string(),
        ok(get("/scenarios").await, "scenarios"),
    )];

    for scenario in scenarios() {
        let story = serde_json::to_value(&scenario.story_plan).unwrap();
        for handler in ["naive", "hardened"] {
            let name = format!("run-{}-{handler}.json", scenario.id);
            let request = json!({ "scenario_id": scenario.id, "seed": SEED, "handler": handler, "fault_plan": story });
            fixtures.push((name.clone(), ok(post("/run", &request).await, &name)));
        }
    }

    let story_run = fixtures
        .iter()
        .find(|(name, _)| name == "run-charge-retry-naive.json")
        .map(|(_, body)| body["replay"].as_str().unwrap().to_string())
        .unwrap();
    fixtures.push((
        "replay.json".to_string(),
        ok(get(&format!("/replay/{story_run}")).await, "replay"),
    ));

    // A generated multi-fault plan, so the shrink fixture shows a reduction.
    let generated = json!({ "scenario_id": "charge-retry", "seed": SEED, "handler": "naive", "fault_plan": null });
    let generated_run = ok(post("/run", &generated).await, "generated run");
    let invariant = failed(&generated_run)[0].clone();
    let shrink = json!({ "scenario_id": "charge-retry", "seed": SEED, "handler": "naive", "fault_plan": null, "invariant": invariant });
    fixtures.push((
        "shrink.json".to_string(),
        ok(post("/shrink", &shrink).await, "shrink"),
    ));

    let sweep = json!({ "scenario_id": "charge-retry", "seed_start": SEED, "count": SWEEP_SEEDS });
    fixtures.push((
        "sweep.json".to_string(),
        ok(post("/sweep", &sweep).await, "sweep"),
    ));
    fixtures
}

#[tokio::test]
async fn frontend_fixtures_match_the_api() {
    let update = env::var("UPDATE_FIXTURES").is_ok_and(|value| value == "1");
    let dir = fixtures_dir();
    let mut stale = Vec::new();
    for (name, body) in canonical_responses().await {
        let path = dir.join(&name);
        let rendered = render(&body);
        if update {
            fs::create_dir_all(&dir).unwrap();
            fs::write(&path, rendered).unwrap();
        } else if fs::read_to_string(&path).ok().as_deref() != Some(rendered.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "frontend fixtures are missing or out of date: {stale:?}. \
         Run `UPDATE_FIXTURES=1 cargo test -p sim-api --test fixtures` and review the diff."
    );
}
