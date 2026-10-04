//! The sweep over every scenario (specs/shrink-sweep-plan.md, SW2). Across
//! every generated plan, the hardened handler never fails and the naive one
//! does: V1's "naive fails often, hardened never" claim.

use sim_core::sweep::{MAX_SWEEP_SEEDS, sweep};
use sim_scenarios::scenarios;

#[test]
fn hardened_never_fails_and_naive_does_on_every_scenario() {
    for scenario in scenarios() {
        let result = sweep(
            &scenario.initial_ledger,
            &scenario.workload,
            0,
            MAX_SWEEP_SEEDS,
        )
        .unwrap_or_else(|error| panic!("{}: {error}", scenario.id));
        assert_eq!(result.hardened_failed, 0, "{}: {result:?}", scenario.id);
        assert!(result.naive_failed > 0, "{}: {result:?}", scenario.id);
    }
}
