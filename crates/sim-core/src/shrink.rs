//! The shrinker (README §6.3). V1 is a single greedy pass: each fault of the
//! original plan is tried for removal once, and the removal is kept if the
//! plan still fails. That isn't full ddmin, and the result isn't guaranteed to
//! be 1-minimal, so it's never called "minimal".

use std::fmt;

use crate::event::SimEvent;
use crate::fault::{FaultOp, FaultPlan};
use crate::handlers::EventHandler;
use crate::simulator::{RunResult, SimError, run};

/// README §6.3's cap of about 500 candidate runs. The greedy pass tries one
/// candidate per fault, so it caps the plan's length.
pub const MAX_SHRINK_FAULTS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct ShrinkResult {
    pub original: FaultPlan,
    pub shrunk: FaultPlan,
    /// The run's own name for the invariant, so nothing borrows the request.
    pub invariant: &'static str,
    /// Candidate plans only: the original check and the final run don't count.
    pub candidates_tried: usize,
    /// The shrunk plan's run, for the UI's "Load reduced run".
    pub run: RunResult,
}

#[derive(Debug)]
pub enum ShrinkError {
    PlanTooLong {
        len: usize,
        max: usize,
    },
    UnknownInvariant(String),
    /// The original plan doesn't fail this invariant, so there's nothing to keep failing.
    DoesNotFail(&'static str),
    Run(SimError),
}

impl fmt::Display for ShrinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShrinkError::PlanTooLong { len, max } => {
                write!(f, "a plan of {len} faults is over the shrink cap of {max}")
            }
            ShrinkError::UnknownInvariant(name) => write!(f, "no invariant is named {name:?}"),
            ShrinkError::DoesNotFail(name) => {
                write!(
                    f,
                    "the plan doesn't fail {name}, so there's nothing to shrink"
                )
            }
            ShrinkError::Run(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ShrinkError {}

/// Shrinks `plan` while the run keeps failing the *same named* invariant
/// (README §6.3). The plan is explicit, never seed-generated: `seed` is only
/// passed through to `run()`, where an explicit plan makes it irrelevant.
pub fn shrink_run(
    initial_ledger: &[(String, i64)],
    workload: &[SimEvent],
    seed: u32,
    plan: &[FaultOp],
    new_handler: &dyn Fn() -> Box<dyn EventHandler>,
    invariant: &str,
) -> Result<ShrinkResult, ShrinkError> {
    if plan.len() > MAX_SHRINK_FAULTS {
        return Err(ShrinkError::PlanTooLong {
            len: plan.len(),
            max: MAX_SHRINK_FAULTS,
        });
    }
    let run_plan = |candidate: &[FaultOp]| {
        run(
            initial_ledger,
            workload,
            seed,
            Some(&candidate.to_vec()),
            new_handler,
        )
        .map_err(ShrinkError::Run)
    };

    let original = run_plan(plan)?;
    let target = original
        .invariants
        .iter()
        .find(|result| result.name == invariant)
        .ok_or_else(|| ShrinkError::UnknownInvariant(invariant.to_string()))?;
    let name = target.name;
    if target.passed {
        return Err(ShrinkError::DoesNotFail(name));
    }

    let (shrunk, candidates_tried) =
        shrink_plan(plan, |candidate| Ok(fails(&run_plan(candidate)?, name)))?;
    let run = run_plan(&shrunk)?;
    Ok(ShrinkResult {
        original: plan.to_vec(),
        shrunk,
        invariant: name,
        candidates_tried,
        run,
    })
}

fn fails(result: &RunResult, invariant: &str) -> bool {
    result
        .invariants
        .iter()
        .any(|check| check.name == invariant && !check.passed)
}

/// Front to back over `plan`. At each position, tries the working plan
/// without that fault. If it still fails, the removal is kept and the next
/// fault, which has shifted into the same position, is tried; otherwise the
/// pass moves on. Returns the shrunk plan and how many candidates were tried,
/// which is always `plan.len()`.
pub fn shrink_plan<E>(
    plan: &[FaultOp],
    mut still_fails: impl FnMut(&[FaultOp]) -> Result<bool, E>,
) -> Result<(FaultPlan, usize), E> {
    let mut kept = plan.to_vec();
    let mut index = 0;
    let mut tried = 0;
    while index < kept.len() {
        let mut candidate = kept.clone();
        candidate.remove(index);
        tried += 1;
        if still_fails(&candidate)? {
            kept = candidate;
        } else {
            index += 1;
        }
    }
    Ok((kept, tried))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::convert::Infallible;

    use super::*;
    use crate::event::{EventId, EventKind};
    use crate::handlers::naive::NaiveHandler;
    use crate::invariants::SINGLE_CAPTURE_PER_INTENT;
    use crate::money::Money;
    use crate::rails::card::{CardEvent, ChargeId};

    /// Distinct faults labelled by number, so plans read as `[1, 2, 3]`.
    fn plan(labels: &[u64]) -> FaultPlan {
        labels
            .iter()
            .map(|&label| FaultOp::Drop {
                event_id: EventId(label),
            })
            .collect()
    }

    fn labels(plan: &[FaultOp]) -> BTreeSet<u64> {
        plan.iter()
            .filter_map(|op| match op {
                FaultOp::Drop { event_id } => Some(event_id.0),
                _ => None,
            })
            .collect()
    }

    /// Shrinks with a predicate over the plan's labels, and counts its calls.
    fn shrink_by(
        original: &[u64],
        fails: impl Fn(&BTreeSet<u64>) -> bool,
    ) -> (FaultPlan, usize, usize) {
        let mut calls = 0;
        let (shrunk, tried) = shrink_plan::<Infallible>(&plan(original), |candidate| {
            calls += 1;
            Ok(fails(&labels(candidate)))
        })
        .unwrap();
        (shrunk, tried, calls)
    }

    #[test]
    fn keeps_exactly_the_faults_the_failure_needs() {
        let (shrunk, tried, _) =
            shrink_by(&[1, 2, 3, 4], |set| set.contains(&2) && set.contains(&4));
        assert_eq!(shrunk, plan(&[2, 4]));
        assert_eq!(tried, 4);
    }

    #[test]
    fn tries_each_fault_exactly_once() {
        for fails in [|_: &BTreeSet<u64>| true, |_: &BTreeSet<u64>| false] {
            let (_, tried, calls) = shrink_by(&[1, 2, 3, 4], fails);
            assert_eq!((tried, calls), (4, 4));
        }
    }

    #[test]
    fn a_plan_whose_every_fault_is_needed_comes_back_unchanged() {
        let (shrunk, _, _) = shrink_by(&[1, 2, 3], |set| set.len() == 3);
        assert_eq!(shrunk, plan(&[1, 2, 3]));
    }

    #[test]
    fn an_empty_plan_tries_nothing() {
        let (shrunk, tried, calls) = shrink_by(&[], |_| true);
        assert_eq!((shrunk, tried, calls), (vec![], 0, 0));
    }

    #[test]
    fn the_result_is_not_always_minimal() {
        // README §6.3's honest limit: fault 1 is tried while it's still
        // needed, and only becomes removable after fault 2 goes. So the pass
        // returns [1, 3] even though [3] alone also fails.
        let failing: [BTreeSet<u64>; 3] = [
            BTreeSet::from([1, 2, 3]),
            BTreeSet::from([1, 3]),
            BTreeSet::from([3]),
        ];
        let (shrunk, _, _) = shrink_by(&[1, 2, 3], |set| failing.contains(set));
        assert_eq!(shrunk, plan(&[1, 3]));
        assert!(failing.contains(&BTreeSet::from([3])));
    }

    #[test]
    fn a_predicate_error_stops_the_pass() {
        let mut calls = 0;
        let result = shrink_plan(&plan(&[1, 2, 3]), |_| {
            calls += 1;
            if calls == 2 {
                Err("run failed")
            } else {
                Ok(true)
            }
        });
        assert_eq!(result, Err("run failed"));
        assert_eq!(calls, 2);
    }

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

    /// Charge 1 captured and partly refunded, and an unrelated charge 3.
    fn workload() -> Vec<SimEvent> {
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
            card(
                3,
                200,
                CardEvent::Captured {
                    charge_id: ChargeId(3),
                    amount: Money(500),
                },
            ),
        ]
    }

    /// Only the duplicate breaks single_capture_per_intent: the delay keeps
    /// the refund after its capture, and the drop hits an unrelated charge.
    fn noisy_plan() -> FaultPlan {
        vec![
            FaultOp::Duplicate {
                event_id: EventId(1),
            },
            FaultOp::Delay {
                event_id: EventId(2),
                by: 50,
            },
            FaultOp::Drop {
                event_id: EventId(3),
            },
        ]
    }

    fn shrink_naive(plan: &[FaultOp], invariant: &str) -> Result<ShrinkResult, ShrinkError> {
        shrink_run(
            &opening(),
            &workload(),
            0,
            plan,
            &|| Box::new(NaiveHandler),
            invariant,
        )
    }

    #[test]
    fn shrink_run_keeps_only_the_fault_that_breaks_the_invariant() {
        let result = shrink_naive(&noisy_plan(), SINGLE_CAPTURE_PER_INTENT).unwrap();
        assert_eq!(result.original, noisy_plan());
        assert_eq!(
            result.shrunk,
            vec![FaultOp::Duplicate {
                event_id: EventId(1)
            }]
        );
        assert_eq!(result.invariant, SINGLE_CAPTURE_PER_INTENT);
        assert_eq!(result.candidates_tried, 3);
        assert_eq!(result.run.fault_plan, result.shrunk);
        assert!(fails(&result.run, SINGLE_CAPTURE_PER_INTENT));
    }

    #[test]
    fn shrink_run_is_deterministic() {
        let first = shrink_naive(&noisy_plan(), SINGLE_CAPTURE_PER_INTENT).unwrap();
        assert_eq!(
            shrink_naive(&noisy_plan(), SINGLE_CAPTURE_PER_INTENT).unwrap(),
            first
        );
    }

    #[test]
    fn a_plan_that_does_not_fail_the_invariant_is_rejected() {
        let delay_only = vec![FaultOp::Delay {
            event_id: EventId(2),
            by: 50,
        }];
        let result = shrink_naive(&delay_only, SINGLE_CAPTURE_PER_INTENT);
        assert!(matches!(
            result,
            Err(ShrinkError::DoesNotFail(SINGLE_CAPTURE_PER_INTENT))
        ));
    }

    #[test]
    fn an_unknown_invariant_is_rejected() {
        let result = shrink_naive(&noisy_plan(), "no_such_invariant");
        assert!(
            matches!(result, Err(ShrinkError::UnknownInvariant(name)) if name == "no_such_invariant")
        );
    }

    #[test]
    fn a_plan_over_the_cap_is_rejected_before_any_run() {
        let long: FaultPlan = (0..=MAX_SHRINK_FAULTS as u64)
            .map(|at| FaultOp::CrashRestart { at })
            .collect();
        let no_runs = || -> Box<dyn EventHandler> { panic!("no run should start") };
        let result = shrink_run(
            &opening(),
            &workload(),
            0,
            &long,
            &no_runs,
            SINGLE_CAPTURE_PER_INTENT,
        );
        assert!(matches!(
            result,
            Err(ShrinkError::PlanTooLong { len, max: MAX_SHRINK_FAULTS }) if len == MAX_SHRINK_FAULTS + 1
        ));
    }
}
