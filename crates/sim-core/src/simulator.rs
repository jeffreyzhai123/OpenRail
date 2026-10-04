use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::clock::{ClockError, VirtualClock};
use crate::event::{EventId, EventQueue, SimEvent};
use crate::fault::{FaultError, FaultPlan, apply_fault_plan, generate_fault_plan};
use crate::handlers::EventHandler;
use crate::invariants::{InvariantContext, InvariantResult, check_all};
use crate::ledger::{JournalEntry, Ledger, LedgerError, LedgerSnapshot};
use crate::money::Money;
use crate::trace::hash_run;

#[derive(Debug)]
pub enum SimError {
    InvalidOpening(LedgerError),
    InvalidFaultPlan(FaultError),
    Clock(ClockError),
    Posting { event: EventId, error: LedgerError },
    TraceEncoding(serde_json::Error),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::InvalidOpening(error) => write!(f, "invalid opening ledger: {error}"),
            SimError::InvalidFaultPlan(error) => write!(f, "invalid fault plan: {error}"),
            SimError::Clock(error) => write!(f, "{error}"),
            SimError::Posting { event, error } => {
                write!(f, "event {} couldn't be posted: {error}", event.0)
            }
            SimError::TraceEncoding(error) => write!(f, "trace couldn't be hashed: {error}"),
        }
    }
}

impl std::error::Error for SimError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunResult {
    pub trace: Vec<SimEvent>,
    pub opening: BTreeMap<String, Money>,
    pub journal: Vec<JournalEntry>,
    pub fault_plan: FaultPlan,
    pub ledger: LedgerSnapshot,
    pub invariants: Vec<InvariantResult>,
    pub trace_hash: String,
}

// README §6.1 shows this taking `scenario: &Scenario`, but `Scenario` is
// defined in sim-scenarios, which depends on sim-core (§2) — not the other
// way around. `new_handler` is a factory so a CrashRestart fault can rebuild
// the handler mid-run with no state surviving by mistake.
pub fn run(
    initial_ledger: &[(String, i64)],
    workload: &[SimEvent],
    seed: u32,
    fault_plan: Option<&FaultPlan>,
    new_handler: &dyn Fn() -> Box<dyn EventHandler>,
) -> Result<RunResult, SimError> {
    let mut ledger = Ledger::open(initial_ledger).map_err(SimError::InvalidOpening)?;

    let plan = match fault_plan {
        Some(plan) => plan.clone(),
        None => generate_fault_plan(seed, workload),
    };
    let schedule = apply_fault_plan(workload, &plan).map_err(SimError::InvalidFaultPlan)?;

    let mut queue = EventQueue::default();
    for delivery in &schedule.deliveries {
        queue.push(delivery.id, delivery.time, delivery.kind.clone());
    }

    let mut pending_crashes = schedule.crashes.into_iter();
    let mut next_crash = pending_crashes.next();

    let mut clock = VirtualClock::default();
    let mut handler = new_handler();
    let mut trace = Vec::new();

    while let Some(event) = queue.pop() {
        while next_crash.is_some_and(|at| at <= event.time) {
            handler = new_handler();
            next_crash = pending_crashes.next();
        }
        clock.advance_to(event.time).map_err(SimError::Clock)?;
        for entry in handler.handle(&event, &ledger) {
            ledger.post(entry).map_err(|error| SimError::Posting {
                event: event.id,
                error,
            })?;
        }
        trace.push(event);
    }

    let invariants = check_all(&InvariantContext { ledger: &ledger });
    let trace_hash = hash_run(&trace, ledger.journal()).map_err(SimError::TraceEncoding)?;

    Ok(RunResult {
        trace,
        opening: ledger.opening().clone(),
        journal: ledger.journal().to_vec(),
        fault_plan: plan,
        ledger: ledger.snapshot(),
        invariants,
        trace_hash,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::event::EventKind;
    use crate::fault::{DUPLICATE_REDELIVERY_MS, FaultOp};
    use crate::handlers::hardened::HardenedHandler;
    use crate::handlers::naive::NaiveHandler;
    use crate::invariants::{
        REFUND_WITHIN_CAPTURE, SINGLE_CAPTURE_PER_INTENT, SINGLE_ENTRY_PER_SOURCE_EVENT,
    };
    use crate::ledger::{EntryKind, IntentId, Posting};
    use crate::rails::card::{CardEvent, ChargeId};

    const CARD: &str = "external:card";
    const MERCHANT: &str = "merchant";

    fn opening() -> Vec<(String, i64)> {
        vec![(CARD.to_string(), 0), (MERCHANT.to_string(), 0)]
    }

    fn captured(id: u64, time: u64) -> SimEvent {
        capture_of(id, time, id)
    }

    fn capture_of(id: u64, time: u64, charge: u64) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq: 0,
            kind: EventKind::Card(CardEvent::Captured {
                charge_id: ChargeId(charge),
                amount: Money(500),
            }),
        }
    }

    fn refunded(id: u64, time: u64, charge: u64) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq: 0,
            kind: EventKind::Card(CardEvent::Refunded {
                charge_id: ChargeId(charge),
                amount: Money(200),
            }),
        }
    }

    fn passed(result: &RunResult, name: &str) -> bool {
        result
            .invariants
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .passed
    }

    fn all_passed(result: &RunResult) -> bool {
        result.invariants.iter().all(|r| r.passed)
    }

    fn run_naive(workload: &[SimEvent], plan: &FaultPlan) -> RunResult {
        run(&opening(), workload, 0, Some(plan), &|| {
            Box::new(NaiveHandler)
        })
        .unwrap()
    }

    fn run_hardened(workload: &[SimEvent], plan: &FaultPlan) -> RunResult {
        run(&opening(), workload, 0, Some(plan), &|| {
            Box::new(HardenedHandler)
        })
        .unwrap()
    }

    /// Dedupes redeliveries in memory only, so a crash-restart forgets them.
    #[derive(Default)]
    struct MemoryDedupHandler {
        seen: BTreeSet<EventId>,
    }

    impl EventHandler for MemoryDedupHandler {
        fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry> {
            if !self.seen.insert(event.id) {
                return vec![];
            }
            NaiveHandler.handle(event, ledger)
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
    fn clean_workload_passes_every_invariant() {
        let workload = [captured(1, 0), refunded(2, 100, 1)];
        let result = run(&opening(), &workload, 0, Some(&FaultPlan::new()), &|| {
            Box::new(NaiveHandler)
        })
        .unwrap();

        assert!(
            result.invariants.iter().all(|r| r.passed),
            "{:?}",
            result.invariants
        );
        assert_eq!(result.journal.len(), 2);
        assert_eq!(
            result.ledger.accounts,
            BTreeMap::from([
                (CARD.to_string(), Money(-300)),
                (MERCHANT.to_string(), Money(300))
            ])
        );
        assert_eq!(
            result.opening,
            BTreeMap::from([
                (CARD.to_string(), Money(0)),
                (MERCHANT.to_string(), Money(0))
            ])
        );
    }

    #[test]
    fn duplicate_capture_breaks_naive_but_not_hardened() {
        let workload = [captured(1, 0)];
        let plan = vec![FaultOp::Duplicate {
            event_id: EventId(1),
        }];

        let naive = run(&opening(), &workload, 0, Some(&plan), &|| {
            Box::new(NaiveHandler)
        })
        .unwrap();
        assert!(!passed(&naive, SINGLE_CAPTURE_PER_INTENT));
        assert!(!passed(&naive, SINGLE_ENTRY_PER_SOURCE_EVENT));

        let hardened = run(&opening(), &workload, 0, Some(&plan), &|| {
            Box::new(HardenedHandler)
        })
        .unwrap();
        assert!(
            hardened.invariants.iter().all(|r| r.passed),
            "{:?}",
            hardened.invariants
        );
    }

    #[test]
    fn crash_restart_replaces_the_handler_mid_run() {
        // The copy of capture 1 arrives DUPLICATE_REDELIVERY_MS after it. A
        // crash in between wipes an in-memory dedupe set, but not the journal.
        let workload = [captured(1, 0)];
        let redelivered = vec![FaultOp::Duplicate {
            event_id: EventId(1),
        }];
        let mut crashed = redelivered.clone();
        crashed.push(FaultOp::CrashRestart {
            at: DUPLICATE_REDELIVERY_MS / 2,
        });
        let memory = || -> Box<dyn EventHandler> { Box::new(MemoryDedupHandler::default()) };

        let calm = run(&opening(), &workload, 0, Some(&redelivered), &memory).unwrap();
        assert!(all_passed(&calm), "{:?}", calm.invariants);

        let after_crash = run(&opening(), &workload, 0, Some(&crashed), &memory).unwrap();
        assert!(!passed(&after_crash, SINGLE_CAPTURE_PER_INTENT));

        let hardened = run_hardened(&workload, &crashed);
        assert!(all_passed(&hardened), "{:?}", hardened.invariants);
    }

    #[test]
    fn delay_and_drop_break_naive_but_not_hardened() {
        // Both leave the refund ahead of any capture.
        let workload = [captured(1, 0), refunded(2, 100, 1)];
        let delayed = vec![FaultOp::Delay {
            event_id: EventId(1),
            by: 200,
        }];
        let dropped = vec![FaultOp::Drop {
            event_id: EventId(1),
        }];
        for plan in [delayed, dropped] {
            assert!(
                !passed(&run_naive(&workload, &plan), REFUND_WITHIN_CAPTURE),
                "{plan:?}"
            );
            let hardened = run_hardened(&workload, &plan);
            assert!(all_passed(&hardened), "{plan:?}: {:?}", hardened.invariants);
        }
    }

    #[test]
    fn a_retried_capture_with_a_new_event_id_breaks_naive_but_not_hardened() {
        // Unlike a redelivery, the retry has its own event id, so only the
        // per-intent invariant catches it.
        let workload = [capture_of(1, 0, 9), capture_of(2, 100, 9)];
        let naive = run_naive(&workload, &FaultPlan::new());
        assert!(!passed(&naive, SINGLE_CAPTURE_PER_INTENT));
        assert!(passed(&naive, SINGLE_ENTRY_PER_SOURCE_EVENT));
        assert!(all_passed(&run_hardened(&workload, &FaultPlan::new())));
    }

    #[test]
    fn trace_is_in_time_order_with_ties_in_workload_order() {
        let workload = [captured(1, 100), captured(2, 100), captured(3, 0)];
        let result = run_naive(&workload, &FaultPlan::new());
        let ids: Vec<u64> = result.trace.iter().map(|event| event.id.0).collect();
        assert_eq!(ids, [3, 1, 2]);
        assert!(
            result
                .trace
                .windows(2)
                .all(|pair| (pair[0].time, pair[0].seq) < (pair[1].time, pair[1].seq))
        );
    }

    #[test]
    fn an_unbalanced_entry_is_a_posting_error_naming_its_event() {
        let result = run(
            &opening(),
            &[captured(7, 0)],
            0,
            Some(&FaultPlan::new()),
            &|| Box::new(UnbalancedHandler),
        );
        assert!(matches!(
            result,
            Err(SimError::Posting {
                event: EventId(7),
                error: LedgerError::Unbalanced { .. },
            })
        ));
    }

    #[test]
    fn invalid_opening_is_an_error() {
        let bad_opening = [(CARD.to_string(), 5)];
        let result = run(&bad_opening, &[], 0, Some(&FaultPlan::new()), &|| {
            Box::new(NaiveHandler)
        });
        assert!(matches!(result, Err(SimError::InvalidOpening(_))));
    }

    #[test]
    fn invalid_fault_plan_is_an_error() {
        let plan = vec![FaultOp::Drop {
            event_id: EventId(99),
        }];
        let result = run(&opening(), &[], 0, Some(&plan), &|| Box::new(NaiveHandler));
        assert!(matches!(result, Err(SimError::InvalidFaultPlan(_))));
    }

    #[test]
    fn empty_workload_gives_an_empty_trace() {
        let result = run(&opening(), &[], 0, Some(&FaultPlan::new()), &|| {
            Box::new(NaiveHandler)
        })
        .unwrap();
        assert_eq!(result.trace, vec![]);
        assert_eq!(result.journal, vec![]);
        assert_eq!(result.trace_hash, hash_run(&[], &[]).unwrap());
    }

    /// The determinism smoke test: the one that must never go yellow.
    #[test]
    fn same_seed_gives_identical_results_a_hundred_times() {
        let workload = [captured(1, 0), refunded(2, 100, 1)];
        let first = run(&opening(), &workload, 42, None, &|| Box::new(NaiveHandler)).unwrap();
        for _ in 0..100 {
            let result = run(&opening(), &workload, 42, None, &|| Box::new(NaiveHandler)).unwrap();
            assert_eq!(result, first);
        }
    }

    #[test]
    fn a_returned_plan_replays_the_run_exactly() {
        // What a share link relies on: the plan in the result, not the seed,
        // determines the run.
        let workload = [captured(1, 0), refunded(2, 100, 1), captured(3, 200)];
        for seed in 0..50 {
            let original = run(&opening(), &workload, seed, None, &|| {
                Box::new(NaiveHandler)
            })
            .unwrap();
            assert_eq!(original.fault_plan, generate_fault_plan(seed, &workload));

            for replay_seed in [seed, seed.wrapping_add(1)] {
                let replay = run(
                    &opening(),
                    &workload,
                    replay_seed,
                    Some(&original.fault_plan),
                    &|| Box::new(NaiveHandler),
                )
                .unwrap();
                assert_eq!(replay, original, "seed {seed}, replayed with {replay_seed}");
            }
        }
    }

    #[test]
    fn full_run_hash_is_pinned() {
        // The cross-process determinism check: the x100 test runs in one
        // process, so it can't see a per-process difference. If this fails,
        // every existing replay link stops verifying: bump the replay-encoding
        // version (README §6.2) before updating the value.
        let workload = [captured(1, 0), refunded(2, 100, 1), captured(3, 200)];
        let plan = vec![
            FaultOp::Delay {
                event_id: EventId(1),
                by: 150,
            },
            FaultOp::Duplicate {
                event_id: EventId(3),
            },
        ];
        assert_eq!(
            run_naive(&workload, &plan).trace_hash,
            "e7446d0498dcdf6b847e7f7691cf3d89c2500618483cb6b98ea14dcfa8f91be6"
        );
    }
}
