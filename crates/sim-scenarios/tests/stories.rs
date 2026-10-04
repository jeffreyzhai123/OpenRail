//! Every scenario end to end through `run()`: its workload alone runs clean,
//! and its story plan breaks the naive handler exactly as described while
//! the hardened handler stays green (specs/scenarios-plan.md).

use std::collections::BTreeSet;

use sim_core::event::EventId;
use sim_core::fault::{FaultOp, FaultPlan};
use sim_core::handlers::HandlerKind;
use sim_core::invariants::{
    REFUND_WITHIN_CAPTURE, SINGLE_CAPTURE_PER_INTENT, SINGLE_ENTRY_PER_SOURCE_EVENT,
};
use sim_core::simulator::{RunResult, run};
use sim_scenarios::{Scenario, scenarios};

/// Every run here passes an explicit plan, so the seed generates nothing.
const SEED: u32 = 0;

/// What each scenario's story plan does to the naive handler.
const NAIVE_FAILS: [(&str, &[&str]); 3] = [
    (
        "charge-retry",
        &[SINGLE_CAPTURE_PER_INTENT, SINGLE_ENTRY_PER_SOURCE_EVENT],
    ),
    ("refund-before-capture", &[REFUND_WITHIN_CAPTURE]),
    (
        "late-ach-return",
        &[REFUND_WITHIN_CAPTURE, SINGLE_ENTRY_PER_SOURCE_EVENT],
    ),
];

fn run_scenario(scenario: &Scenario, plan: &FaultPlan, kind: HandlerKind) -> RunResult {
    run(
        &scenario.initial_ledger,
        &scenario.workload,
        SEED,
        Some(plan),
        &|| kind.build(),
    )
    .unwrap_or_else(|error| panic!("{} under {kind:?}: {error}", scenario.id))
}

fn failed(result: &RunResult) -> BTreeSet<&'static str> {
    result
        .invariants
        .iter()
        .filter(|invariant| !invariant.passed)
        .map(|invariant| invariant.name)
        .collect()
}

#[test]
fn every_scenario_has_an_expected_story() {
    let expected: BTreeSet<&str> = NAIVE_FAILS.iter().map(|(id, _)| *id).collect();
    let registered: BTreeSet<&str> = scenarios().iter().map(|scenario| scenario.id).collect();
    assert_eq!(expected, registered);
}

#[test]
fn workloads_alone_run_clean_under_both_handlers() {
    for scenario in scenarios() {
        for kind in [HandlerKind::Naive, HandlerKind::Hardened] {
            let result = run_scenario(&scenario, &FaultPlan::new(), kind);
            assert_eq!(
                failed(&result),
                BTreeSet::new(),
                "{} under {kind:?}",
                scenario.id
            );
        }
    }
}

#[test]
fn story_plans_break_naive_exactly_as_described() {
    for (id, expected) in NAIVE_FAILS {
        let scenario = sim_scenarios::find(id).unwrap();
        let result = run_scenario(&scenario, &scenario.story_plan, HandlerKind::Naive);
        assert_eq!(failed(&result), expected.iter().copied().collect(), "{id}");
    }
}

#[test]
fn story_plans_leave_hardened_green() {
    for scenario in scenarios() {
        let result = run_scenario(&scenario, &scenario.story_plan, HandlerKind::Hardened);
        assert_eq!(failed(&result), BTreeSet::new(), "{}", scenario.id);
    }
}

#[test]
fn hardened_survives_a_dropped_ach_initiation() {
    // The return then arrives for a debit that never posted. Naive reverses
    // money that never moved in; hardened rejects the return, as it does a
    // card refund before its capture (D4).
    let scenario = sim_scenarios::find("late-ach-return").unwrap();
    let plan = vec![FaultOp::Drop {
        event_id: EventId(1),
    }];

    let naive = run_scenario(&scenario, &plan, HandlerKind::Naive);
    assert_eq!(failed(&naive), BTreeSet::from([REFUND_WITHIN_CAPTURE]));

    let hardened = run_scenario(&scenario, &plan, HandlerKind::Hardened);
    assert_eq!(failed(&hardened), BTreeSet::new());
}
