//! The wire types. They serialize in the order `frontend-plan.md`'s "API
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
