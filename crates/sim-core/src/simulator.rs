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
    use super::*;
    use crate::event::EventKind;
    use crate::fault::FaultOp;
    use crate::handlers::hardened::HardenedHandler;
    use crate::handlers::naive::NaiveHandler;
    use crate::invariants::{SINGLE_CAPTURE_PER_INTENT, SINGLE_ENTRY_PER_SOURCE_EVENT};
    use crate::rails::card::{CardEvent, ChargeId};

    const CARD: &str = "external:card";
    const MERCHANT: &str = "merchant";

    fn opening() -> Vec<(String, i64)> {
        vec![(CARD.to_string(), 0), (MERCHANT.to_string(), 0)]
    }

    fn captured(id: u64, time: u64) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq: 0,
            kind: EventKind::Card(CardEvent::Captured {
                charge_id: ChargeId(id),
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
    fn crash_restart_does_not_break_hardened() {
        let workload = [captured(1, 0)];
        let plan = vec![
            FaultOp::Duplicate {
                event_id: EventId(1),
            },
            FaultOp::CrashRestart { at: 0 },
        ];
        let result = run(&opening(), &workload, 0, Some(&plan), &|| {
            Box::new(HardenedHandler)
        })
        .unwrap();
        assert!(
            result.invariants.iter().all(|r| r.passed),
            "{:?}",
            result.invariants
        );
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
}
