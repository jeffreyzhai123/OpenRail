//! The shrinker (README §6.3). V1 is a single greedy pass: each fault of the
//! original plan is tried for removal once, and the removal is kept if the
//! plan still fails. That isn't full ddmin, and the result isn't guaranteed to
//! be 1-minimal, so it's never called "minimal".

use crate::fault::{FaultOp, FaultPlan};

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
    use crate::event::EventId;

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
}
