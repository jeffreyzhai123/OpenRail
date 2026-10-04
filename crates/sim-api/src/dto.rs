//! The wire types. They serialize in the order `v1-frontend-tasks.md`'s "API
//! contract v1" lists their fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sim_core::event::SimEvent;
use sim_core::fault::FaultPlan;
use sim_core::handlers::HandlerKind;
use sim_core::invariants::InvariantResult;
use sim_core::ledger::{JournalEntry, LedgerSnapshot};
use sim_core::money::Money;
use sim_core::simulator::RunResult;
use sim_core::sweep::SweepResult;
use sim_scenarios::Scenario;

use crate::encode::{Replay, encode_run};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    pub(crate) scenario_id: String,
    pub(crate) seed: u32,
    pub(crate) handler: HandlerKind,
    /// `null`, or absent, means "generate a plan from the seed".
    pub(crate) fault_plan: Option<FaultPlan>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RunResponse {
    scenario_id: String,
    seed: u32,
    handler: HandlerKind,
    /// The effective plan: the request's, or the one generated from the seed.
    fault_plan: FaultPlan,
    trace: Vec<SimEvent>,
    /// Aligned with `trace`: the journal entries each delivered event posted.
    posted: Vec<usize>,
    opening: BTreeMap<String, Money>,
    journal: Vec<JournalEntry>,
    ledger: LedgerSnapshot,
    invariants: Vec<InvariantResult>,
    trace_hash: String,
    replay: String,
}

impl RunResponse {
    /// The replay link encodes the effective plan, so it replays this exact
    /// run without regenerating anything.
    pub(crate) fn new(
        scenario_id: String,
        seed: u32,
        handler: HandlerKind,
        result: RunResult,
    ) -> Result<Self, serde_json::Error> {
        let replay = encode_run(&Replay {
            scenario_id: scenario_id.clone(),
            seed,
            handler,
            fault_plan: result.fault_plan.clone(),
        })?;
        Ok(RunResponse {
            scenario_id,
            seed,
            handler,
            fault_plan: result.fault_plan,
            trace: result.trace,
            posted: result.posted,
            opening: result.opening,
            journal: result.journal,
            ledger: result.ledger,
            invariants: result.invariants,
            trace_hash: result.trace_hash,
            replay,
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ScenarioSummary {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    /// The opening's account names, which the UI's balance panel lists.
    accounts: Vec<String>,
    workload: Vec<SimEvent>,
    story_plan: FaultPlan,
}

impl From<Scenario> for ScenarioSummary {
    fn from(scenario: Scenario) -> Self {
        ScenarioSummary {
            id: scenario.id,
            name: scenario.name,
            description: scenario.description,
            accounts: scenario
                .initial_ledger
                .into_iter()
                .map(|(account, _)| account)
                .collect(),
            workload: scenario.workload,
            story_plan: scenario.story_plan,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShrinkRequest {
    pub(crate) scenario_id: String,
    pub(crate) seed: u32,
    pub(crate) handler: HandlerKind,
    /// `null`, or absent, means the seed's generated plan, the one `POST /run`
    /// would use.
    pub(crate) fault_plan: Option<FaultPlan>,
    pub(crate) invariant: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShrinkResponse {
    pub(crate) original: FaultPlan,
    pub(crate) shrunk: FaultPlan,
    pub(crate) invariant: &'static str,
    pub(crate) candidates_tried: usize,
    /// The shrunk plan's run, with its own replay link.
    pub(crate) run: RunResponse,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SweepRequest {
    pub(crate) scenario_id: String,
    pub(crate) seed_start: u32,
    pub(crate) count: u32,
}

/// Counts, not rates: the UI computes the percentages.
#[derive(Debug, Serialize)]
pub(crate) struct SweepResponse {
    count: u32,
    naive: Failures,
    hardened: Failures,
}

#[derive(Debug, Serialize)]
struct Failures {
    failed: u32,
}

impl From<SweepResult> for SweepResponse {
    fn from(result: SweepResult) -> Self {
        SweepResponse {
            count: result.runs,
            naive: Failures {
                failed: result.naive_failed,
            },
            hardened: Failures {
                failed: result.hardened_failed,
            },
        }
    }
}
