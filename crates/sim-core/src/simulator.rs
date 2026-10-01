use crate::event::SimEvent;
use crate::fault::FaultPlan;
use crate::invariants::InvariantResult;
use crate::ledger::LedgerSnapshot;

pub struct RunResult {
    pub trace: Vec<SimEvent>,
    pub ledger: LedgerSnapshot,
    pub invariants: Vec<InvariantResult>,
    pub trace_hash: String,
}

// README §6.1 shows this taking `scenario: &Scenario`, but `Scenario` is
// defined in sim-scenarios, which depends on sim-core (§2) — not the
// other way around. Taking initial_ledger/workload directly avoids the
// circular dependency; sim-scenarios can destructure its `Scenario` when
// calling this.
pub fn run(
    initial_ledger: &[(String, i64)],
    workload: &[SimEvent],
    seed: u64,
    fault_plan: Option<&FaultPlan>,
) -> RunResult {
    let _ = (initial_ledger, workload, seed, fault_plan);
    unimplemented!()
}
