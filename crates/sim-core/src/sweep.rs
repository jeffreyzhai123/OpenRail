//! The sweep harness (README §3): naive vs. hardened over a range of seeds,
//! each run with its seed's generated fault plan. It counts the runs that
//! fail any invariant, which the UI turns into a failure rate per handler.

use std::fmt;

use crate::event::SimEvent;
use crate::handlers::{EventHandler, HandlerKind};
use crate::simulator::{SimError, run};

/// Caps one sweep's work. Each seed costs two runs, one per handler.
pub const MAX_SWEEP_SEEDS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepResult {
    pub runs: u32,
    pub naive_failed: u32,
    pub hardened_failed: u32,
}

#[derive(Debug)]
pub enum SweepError {
    TooManySeeds {
        count: u32,
        max: u32,
    },
    /// The last seed, `seed_start + count - 1`, would pass `u32::MAX`.
    SeedOverflow,
    Run {
        seed: u32,
        error: SimError,
    },
}

impl fmt::Display for SweepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SweepError::TooManySeeds { count, max } => {
                write!(f, "a sweep of {count} seeds is over the cap of {max}")
            }
            SweepError::SeedOverflow => write!(f, "the sweep's last seed would pass u32::MAX"),
            SweepError::Run { seed, error } => write!(f, "seed {seed}: {error}"),
        }
    }
}

impl std::error::Error for SweepError {}

/// Runs naive and hardened for every seed in `seed_start..seed_start + count`.
pub fn sweep(
    initial_ledger: &[(String, i64)],
    workload: &[SimEvent],
    seed_start: u32,
    count: u32,
) -> Result<SweepResult, SweepError> {
    if count > MAX_SWEEP_SEEDS {
        return Err(SweepError::TooManySeeds {
            count,
            max: MAX_SWEEP_SEEDS,
        });
    }
    if count > 0 && seed_start.checked_add(count - 1).is_none() {
        return Err(SweepError::SeedOverflow);
    }
    // Checked above: every seed fits in a u32.
    let seeds = || (0..count).map(move |offset| seed_start + offset);

    Ok(SweepResult {
        runs: count,
        naive_failed: count_failing_runs(initial_ledger, workload, seeds(), &|| {
            HandlerKind::Naive.build()
        })?,
        hardened_failed: count_failing_runs(initial_ledger, workload, seeds(), &|| {
            HandlerKind::Hardened.build()
        })?,
    })
}

/// How many runs fail any invariant, one per seed, each with its seed's
/// generated plan. A `run()` error stops the count: generated plans are always
/// valid, so it means a handler posted a broken entry, which is a bug to show
/// rather than a failure to count.
pub fn count_failing_runs(
    initial_ledger: &[(String, i64)],
    workload: &[SimEvent],
    seeds: impl IntoIterator<Item = u32>,
    new_handler: &dyn Fn() -> Box<dyn EventHandler>,
) -> Result<u32, SweepError> {
    let mut failed = 0;
    for seed in seeds {
        let result = run(initial_ledger, workload, seed, None, new_handler)
            .map_err(|error| SweepError::Run { seed, error })?;
        if result.invariants.iter().any(|invariant| !invariant.passed) {
            failed += 1;
        }
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventId, EventKind};
    use crate::ledger::{EntryKind, IntentId, JournalEntry, Ledger, Posting};
    use crate::money::Money;
    use crate::rails::card::{CardEvent, ChargeId};

    const CARD: &str = "external:card";
    const MERCHANT: &str = "merchant";

    fn opening() -> Vec<(String, i64)> {
        vec![(CARD.to_string(), 0), (MERCHANT.to_string(), 0)]
    }

    fn card(id: u64, time: u64, event: CardEvent) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq: 0,
            kind: EventKind::Card(event),
        }
    }

    /// A capture, then a partial refund 100 ms later. Most faults on either
    /// event break the naive handler.
    fn capture_then_refund() -> Vec<SimEvent> {
        vec![
            card(
                1,
                0,
                CardEvent::Captured {
                    charge_id: ChargeId(1),
                    amount: Money(500),
                },
            ),
            card(
                2,
                100,
                CardEvent::Refunded {
                    charge_id: ChargeId(1),
                    amount: Money(200),
                },
            ),
        ]
    }

    /// Never posts anything, so no invariant can fail.
    struct InertHandler;

    impl EventHandler for InertHandler {
        fn handle(&mut self, _event: &SimEvent, _ledger: &Ledger) -> Vec<JournalEntry> {
            vec![]
        }
    }

    /// Emits a one-legged entry, which can't balance.
    struct UnbalancedHandler;

    impl EventHandler for UnbalancedHandler {
        fn handle(&mut self, event: &SimEvent, _ledger: &Ledger) -> Vec<JournalEntry> {
            vec![JournalEntry {
                source: event.id,
                intent: IntentId("broken".to_string()),
                kind: EntryKind::Capture,
                postings: vec![Posting {
                    account: MERCHANT.to_string(),
                    delta: Money(1),
                }],
            }]
        }
    }

    #[test]
    fn naive_fails_some_seeds_and_hardened_none() {
        let result = sweep(&opening(), &capture_then_refund(), 0, 200).unwrap();
        assert_eq!(result.runs, 200);
        assert!(result.naive_failed > 0, "{result:?}");
        assert!(result.naive_failed <= result.runs, "{result:?}");
        assert_eq!(result.hardened_failed, 0, "{result:?}");
    }

    #[test]
    fn same_arguments_give_the_same_result() {
        let first = sweep(&opening(), &capture_then_refund(), 7, 100).unwrap();
        assert_eq!(
            sweep(&opening(), &capture_then_refund(), 7, 100).unwrap(),
            first
        );
    }

    #[test]
    fn a_handler_that_never_posts_never_fails() {
        let failed = count_failing_runs(&opening(), &capture_then_refund(), 0..100, &|| {
            Box::new(InertHandler)
        })
        .unwrap();
        assert_eq!(failed, 0);
    }

    #[test]
    fn an_empty_sweep_runs_nothing() {
        let result = sweep(&opening(), &capture_then_refund(), 0, 0).unwrap();
        assert_eq!(
            result,
            SweepResult {
                runs: 0,
                naive_failed: 0,
                hardened_failed: 0
            }
        );
    }

    #[test]
    fn too_many_seeds_is_rejected() {
        let result = sweep(&opening(), &capture_then_refund(), 0, MAX_SWEEP_SEEDS + 1);
        assert!(matches!(
            result,
            Err(SweepError::TooManySeeds { count, max: MAX_SWEEP_SEEDS }) if count == MAX_SWEEP_SEEDS + 1
        ));
    }

    #[test]
    fn the_last_seed_may_be_u32_max_but_not_past_it() {
        let result = sweep(&opening(), &capture_then_refund(), u32::MAX, 1).unwrap();
        assert_eq!(result.runs, 1);
        let result = sweep(&opening(), &capture_then_refund(), u32::MAX, 2);
        assert!(matches!(result, Err(SweepError::SeedOverflow)));
    }

    #[test]
    fn a_run_error_stops_the_sweep_and_names_its_seed() {
        let result = count_failing_runs(&opening(), &capture_then_refund(), 5..10, &|| {
            Box::new(UnbalancedHandler)
        });
        assert!(matches!(
            result,
            Err(SweepError::Run {
                seed: 5,
                error: SimError::Posting { .. }
            })
        ));
    }
}
