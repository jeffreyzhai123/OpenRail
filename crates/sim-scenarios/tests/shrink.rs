//! The shrinker on the real scenarios (specs/shrink-sweep-plan.md, SK2).

use sim_core::fault::{FaultPlan, generate_fault_plan};
use sim_core::handlers::HandlerKind;
use sim_core::shrink::{ShrinkResult, shrink_run};
use sim_core::simulator::{RunResult, run};
use sim_core::sweep::MAX_SWEEP_SEEDS;
use sim_scenarios::{Scenario, scenarios};

/// Story plans are explicit, so the seed generates nothing.
const STORY_SEED: u32 = 0;

fn run_naive(scenario: &Scenario, seed: u32, plan: &FaultPlan) -> RunResult {
    run(
        &scenario.initial_ledger,
        &scenario.workload,
        seed,
        Some(plan),
        &|| HandlerKind::Naive.build(),
    )
    .unwrap_or_else(|error| panic!("{}: {error}", scenario.id))
}

fn shrink_naive(scenario: &Scenario, seed: u32, plan: &FaultPlan, invariant: &str) -> ShrinkResult {
    shrink_run(
        &scenario.initial_ledger,
        &scenario.workload,
        seed,
        plan,
        &|| HandlerKind::Naive.build(),
        invariant,
    )
    .unwrap_or_else(|error| panic!("{}: {error}", scenario.id))
}

fn first_failed(result: &RunResult) -> Option<&'static str> {
    result
        .invariants
        .iter()
        .find(|invariant| !invariant.passed)
        .map(|invariant| invariant.name)
}

fn fails(result: &RunResult, name: &str) -> bool {
    result
        .invariants
        .iter()
        .any(|invariant| invariant.name == name && !invariant.passed)
}

#[test]
fn story_plans_do_not_shrink() {
    // Every story fault is needed: without it, the workload runs clean.
    for scenario in scenarios() {
        let story = run_naive(&scenario, STORY_SEED, &scenario.story_plan);
        let invariant = first_failed(&story).expect("a story plan breaks naive");
        let result = shrink_naive(&scenario, STORY_SEED, &scenario.story_plan, invariant);
        assert_eq!(result.shrunk, scenario.story_plan, "{}", scenario.id);
        assert_eq!(result.candidates_tried, scenario.story_plan.len());
    }
}

#[test]
fn failing_sweep_plans_shrink_and_still_fail() {
    let mut any_reduced = false;
    for scenario in scenarios() {
        // The first seed whose generated plan has several faults and breaks
        // naive: the kind of plan a user would actually shrink.
        let (seed, plan, invariant) = (0..MAX_SWEEP_SEEDS)
            .find_map(|seed| {
                let plan = generate_fault_plan(seed, &scenario.workload);
                if plan.len() < 2 {
                    return None;
                }
                first_failed(&run_naive(&scenario, seed, &plan)).map(|name| (seed, plan, name))
            })
            .unwrap_or_else(|| panic!("{}: no failing multi-fault seed", scenario.id));

        let result = shrink_naive(&scenario, seed, &plan, invariant);
        assert!(result.shrunk.len() <= plan.len(), "{}", scenario.id);
        assert!(
            fails(&result.run, invariant),
            "{}: the shrunk plan no longer fails {invariant}",
            scenario.id
        );
        any_reduced |= result.shrunk.len() < plan.len();
    }
    assert!(any_reduced, "no fixture actually shrank");
}
