//! Fault injection. A `FaultPlan` is data, and applying it is a pure function
//! with no randomness, so the shrinker can remove one fault without changing
//! what the others do (README §6.2). The seed is only used to generate an
//! initial plan.
//!
//! What each op does is part of the replay contract: a replay link stores the
//! plan, so changing an op's effect breaks old links.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::iter;

use serde::{Deserialize, Serialize};

use crate::event::{EventId, EventKind, SimEvent};

/// A provider redelivering a webhook after its delivery timed out.
pub const DUPLICATE_REDELIVERY_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultOp {
    /// A copy with the same `EventId` arrives `DUPLICATE_REDELIVERY_MS` after
    /// the (possibly delayed) original.
    Duplicate { event_id: EventId },
    /// In delivery order, the `window` deliveries starting at `event_id`
    /// arrive in reverse. Each takes the time slot of the one it swaps with.
    Reorder { event_id: EventId, window: usize },
    /// The event arrives `by` ms later.
    Delay { event_id: EventId, by: u64 },
    /// The event is never delivered.
    Drop { event_id: EventId },
    /// Before the first delivery at or after `at` ms, the handler is replaced
    /// with a fresh one. The ledger survives: it's the durable store.
    CrashRestart { at: u64 },
}

pub type FaultPlan = Vec<FaultOp>;

#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub id: EventId,
    pub time: u64,
    pub kind: EventKind,
}

/// What a run delivers once a plan is applied.
#[derive(Debug, PartialEq)]
pub struct Schedule {
    /// In delivery order: by time, ties in workload order, copies after
    /// originals.
    pub deliveries: Vec<Delivery>,
    /// `CrashRestart` times, sorted.
    pub crashes: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultError {
    UnknownEvent(EventId),
    DuplicateWorkloadId(EventId),
    TimeOverflow(EventId),
}

impl fmt::Display for FaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaultError::UnknownEvent(id) => {
                write!(
                    f,
                    "fault targets event {}, which isn't in the workload",
                    id.0
                )
            }
            FaultError::DuplicateWorkloadId(id) => {
                write!(f, "workload has more than one event with id {}", id.0)
            }
            FaultError::TimeOverflow(id) => {
                write!(f, "fault pushes event {} past the end of time", id.0)
            }
        }
    }
}

impl std::error::Error for FaultError {}

/// Applies the ops in fixed phases (Drop, Delay, Duplicate, then Reorder), so
/// an op means the same thing wherever it sits in the plan. Only the relative
/// order of `Reorder`s matters.
pub fn apply_fault_plan(workload: &[SimEvent], plan: &[FaultOp]) -> Result<Schedule, FaultError> {
    check_ids(workload, plan)?;

    let mut deliveries: Vec<Delivery> = workload
        .iter()
        .map(|event| Delivery {
            id: event.id,
            time: event.time,
            kind: event.kind.clone(),
        })
        .collect();

    for op in plan {
        if let FaultOp::Drop { event_id } = op {
            drop_event(&mut deliveries, *event_id);
        }
    }
    for op in plan {
        if let FaultOp::Delay { event_id, by } = op {
            delay_event(&mut deliveries, *event_id, *by)?;
        }
    }
    duplicate_events(&mut deliveries, plan)?;
    // Stable on purpose: same-time deliveries keep workload order, copies last.
    deliveries.sort_by_key(|delivery| delivery.time);
    for op in plan {
        if let FaultOp::Reorder { event_id, window } = op {
            reverse_window(&mut deliveries, *event_id, *window);
        }
    }

    let mut crashes: Vec<u64> = plan
        .iter()
        .filter_map(|op| match op {
            FaultOp::CrashRestart { at } => Some(*at),
            _ => None,
        })
        .collect();
    crashes.sort_unstable();

    Ok(Schedule {
        deliveries,
        crashes,
    })
}

/// Ops find events by id, so ids must be unique and every target must exist.
fn check_ids(workload: &[SimEvent], plan: &[FaultOp]) -> Result<(), FaultError> {
    let mut ids = BTreeSet::new();
    for event in workload {
        if !ids.insert(event.id) {
            return Err(FaultError::DuplicateWorkloadId(event.id));
        }
    }
    match plan.iter().filter_map(target).find(|id| !ids.contains(id)) {
        Some(unknown) => Err(FaultError::UnknownEvent(unknown)),
        None => Ok(()),
    }
}

fn target(op: &FaultOp) -> Option<EventId> {
    match op {
        FaultOp::Duplicate { event_id }
        | FaultOp::Reorder { event_id, .. }
        | FaultOp::Delay { event_id, .. }
        | FaultOp::Drop { event_id } => Some(*event_id),
        FaultOp::CrashRestart { .. } => None,
    }
}

fn drop_event(deliveries: &mut Vec<Delivery>, event_id: EventId) {
    deliveries.retain(|delivery| delivery.id != event_id);
}

fn delay_event(deliveries: &mut [Delivery], event_id: EventId, by: u64) -> Result<(), FaultError> {
    for delivery in deliveries.iter_mut().filter(|d| d.id == event_id) {
        delivery.time = delivery
            .time
            .checked_add(by)
            .ok_or(FaultError::TimeOverflow(event_id))?;
    }
    Ok(())
}

/// Handles every `Duplicate` at once, so copies follow their originals'
/// order rather than plan order.
fn duplicate_events(deliveries: &mut Vec<Delivery>, plan: &[FaultOp]) -> Result<(), FaultError> {
    let mut copies_per_event: BTreeMap<EventId, usize> = BTreeMap::new();
    for op in plan {
        if let FaultOp::Duplicate { event_id } = op {
            *copies_per_event.entry(*event_id).or_default() += 1;
        }
    }

    let mut copies = Vec::new();
    for original in deliveries.iter() {
        let Some(&count) = copies_per_event.get(&original.id) else {
            continue;
        };
        let time = original
            .time
            .checked_add(DUPLICATE_REDELIVERY_MS)
            .ok_or(FaultError::TimeOverflow(original.id))?;
        let copy = Delivery {
            time,
            ..original.clone()
        };
        copies.extend(iter::repeat_n(copy, count));
    }
    deliveries.extend(copies);
    Ok(())
}

/// Reverses which events arrive in the window but keeps its time slots, so
/// the schedule stays sorted. Does nothing if the event was dropped.
fn reverse_window(deliveries: &mut [Delivery], event_id: EventId, window: usize) {
    let Some(start) = deliveries.iter().position(|d| d.id == event_id) else {
        return;
    };
    let end = start.saturating_add(window).min(deliveries.len());
    let slots = &mut deliveries[start..end];
    let times: Vec<u64> = slots.iter().map(|delivery| delivery.time).collect();
    slots.reverse();
    for (delivery, time) in slots.iter_mut().zip(times) {
        delivery.time = time;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Money;
    use crate::rails::card::{CardEvent, ChargeId};
    use crate::rng::Rng;
    // Named imports: the prelude glob also exports a `Rng` trait.
    use proptest::prelude::{
        Strategy, any, prop, prop_assert, prop_assert_eq, prop_oneof, proptest,
    };

    fn captured(id: u64, time: u64) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq: 0,
            kind: EventKind::Card(CardEvent::Captured {
                charge_id: ChargeId(id),
                amount: Money(100),
            }),
        }
    }

    /// Out of time order in the slice, with a tie at 3,000 ms between
    /// events 3 and 4.
    fn workload() -> Vec<SimEvent> {
        vec![
            captured(1, 1_000),
            captured(3, 3_000),
            captured(2, 2_000),
            captured(4, 3_000),
        ]
    }

    fn apply(plan: &[FaultOp]) -> Schedule {
        apply_fault_plan(&workload(), plan).unwrap()
    }

    fn ids(schedule: &Schedule) -> Vec<u64> {
        schedule.deliveries.iter().map(|d| d.id.0).collect()
    }

    fn times(schedule: &Schedule) -> Vec<u64> {
        schedule.deliveries.iter().map(|d| d.time).collect()
    }

    #[test]
    fn empty_plan_delivers_the_workload_in_time_order() {
        let schedule = apply(&[]);
        assert_eq!(ids(&schedule), [1, 2, 3, 4]);
        assert_eq!(times(&schedule), [1_000, 2_000, 3_000, 3_000]);
        assert!(schedule.crashes.is_empty());
    }

    #[test]
    fn drop_removes_the_event() {
        let schedule = apply(&[FaultOp::Drop {
            event_id: EventId(2),
        }]);
        assert_eq!(ids(&schedule), [1, 3, 4]);
    }

    #[test]
    fn delay_moves_the_event_past_later_ones() {
        let schedule = apply(&[FaultOp::Delay {
            event_id: EventId(1),
            by: 1_500,
        }]);
        assert_eq!(ids(&schedule), [2, 1, 3, 4]);
        assert_eq!(times(&schedule), [2_000, 2_500, 3_000, 3_000]);
    }

    #[test]
    fn duplicate_adds_a_same_id_copy_after_the_redelivery_delay() {
        let schedule = apply(&[FaultOp::Duplicate {
            event_id: EventId(1),
        }]);
        assert_eq!(ids(&schedule), [1, 2, 3, 4, 1]);
        let copy = &schedule.deliveries[4];
        assert_eq!(copy.time, 1_000 + DUPLICATE_REDELIVERY_MS);
        assert_eq!(copy.kind, schedule.deliveries[0].kind);
    }

    #[test]
    fn duplicate_copies_a_delayed_original() {
        let schedule = apply(&[
            FaultOp::Duplicate {
                event_id: EventId(1),
            },
            FaultOp::Delay {
                event_id: EventId(1),
                by: 500,
            },
        ]);
        let copy_time = schedule.deliveries.last().map(|d| d.time);
        assert_eq!(copy_time, Some(1_500 + DUPLICATE_REDELIVERY_MS));
    }

    #[test]
    fn copies_follow_their_originals_order_not_plan_order() {
        let schedule = apply(&[
            FaultOp::Duplicate {
                event_id: EventId(4),
            },
            FaultOp::Duplicate {
                event_id: EventId(3),
            },
        ]);
        assert_eq!(ids(&schedule), [1, 2, 3, 4, 3, 4]);
    }

    #[test]
    fn reorder_window_of_two_swaps_events_and_keeps_time_slots() {
        let schedule = apply(&[FaultOp::Reorder {
            event_id: EventId(1),
            window: 2,
        }]);
        assert_eq!(ids(&schedule), [2, 1, 3, 4]);
        assert_eq!(times(&schedule), [1_000, 2_000, 3_000, 3_000]);
    }

    #[test]
    fn reorder_window_of_three_reverses_them() {
        let schedule = apply(&[FaultOp::Reorder {
            event_id: EventId(2),
            window: 3,
        }]);
        assert_eq!(ids(&schedule), [1, 4, 3, 2]);
        assert_eq!(times(&schedule), [1_000, 2_000, 3_000, 3_000]);
    }

    #[test]
    fn reorder_window_is_clamped_at_the_end() {
        let schedule = apply(&[FaultOp::Reorder {
            event_id: EventId(3),
            window: 10,
        }]);
        assert_eq!(ids(&schedule), [1, 2, 4, 3]);
    }

    #[test]
    fn reorder_windows_of_zero_and_one_do_nothing() {
        for window in [0, 1] {
            let schedule = apply(&[FaultOp::Reorder {
                event_id: EventId(2),
                window,
            }]);
            assert_eq!(schedule, apply(&[]), "window {window}");
        }
    }

    #[test]
    fn crash_restarts_come_back_sorted_and_leave_deliveries_alone() {
        let schedule = apply(&[
            FaultOp::CrashRestart { at: 5_000 },
            FaultOp::CrashRestart { at: 0 },
        ]);
        assert_eq!(schedule.crashes, [0, 5_000]);
        assert_eq!(schedule.deliveries, apply(&[]).deliveries);
    }

    #[test]
    fn ops_on_a_dropped_event_do_nothing() {
        let id = EventId(2);
        let schedule = apply(&[
            FaultOp::Duplicate { event_id: id },
            FaultOp::Delay {
                event_id: id,
                by: 10,
            },
            FaultOp::Reorder {
                event_id: id,
                window: 3,
            },
            FaultOp::Drop { event_id: id },
        ]);
        assert_eq!(schedule, apply(&[FaultOp::Drop { event_id: id }]));
    }

    #[test]
    fn an_op_on_an_unknown_event_is_rejected() {
        let unknown = EventId(99);
        let ops = [
            FaultOp::Duplicate { event_id: unknown },
            FaultOp::Reorder {
                event_id: unknown,
                window: 2,
            },
            FaultOp::Delay {
                event_id: unknown,
                by: 1,
            },
            FaultOp::Drop { event_id: unknown },
        ];
        for op in ops {
            assert_eq!(
                apply_fault_plan(&workload(), std::slice::from_ref(&op)),
                Err(FaultError::UnknownEvent(unknown)),
                "{op:?}"
            );
        }
    }

    #[test]
    fn duplicate_workload_ids_are_rejected() {
        let workload = [captured(1, 0), captured(1, 10)];
        assert_eq!(
            apply_fault_plan(&workload, &[]),
            Err(FaultError::DuplicateWorkloadId(EventId(1)))
        );
    }

    #[test]
    fn overflowing_delay_is_rejected() {
        let plan = [FaultOp::Delay {
            event_id: EventId(1),
            by: u64::MAX,
        }];
        assert_eq!(
            apply_fault_plan(&workload(), &plan),
            Err(FaultError::TimeOverflow(EventId(1)))
        );
    }

    #[test]
    fn overflowing_redelivery_is_rejected() {
        let workload = [captured(1, u64::MAX - 1)];
        let plan = [FaultOp::Duplicate {
            event_id: EventId(1),
        }];
        assert_eq!(
            apply_fault_plan(&workload, &plan),
            Err(FaultError::TimeOverflow(EventId(1)))
        );
    }

    const MAX_EVENTS: u64 = 8;

    /// A workload with ids `1..=len` at times drawn from a small range, so
    /// ties are common, plus a plan whose ops all target those ids.
    fn case() -> impl Strategy<Value = (Vec<SimEvent>, Vec<FaultOp>)> {
        (1..=MAX_EVENTS)
            .prop_flat_map(|len| {
                let id = (1..=len).prop_map(EventId);
                let op = prop_oneof![
                    id.clone().prop_map(|event_id| FaultOp::Drop { event_id }),
                    (id.clone(), 0u64..5_000)
                        .prop_map(|(event_id, by)| FaultOp::Delay { event_id, by }),
                    id.clone()
                        .prop_map(|event_id| FaultOp::Duplicate { event_id }),
                    (id, 0usize..5)
                        .prop_map(|(event_id, window)| FaultOp::Reorder { event_id, window }),
                    (0u64..10_000).prop_map(|at| FaultOp::CrashRestart { at }),
                ];
                let times = prop::collection::vec(0u64..5_000, len as usize);
                (times, prop::collection::vec(op, 0..8))
            })
            .prop_map(|(times, plan)| {
                let workload = (1..)
                    .zip(times)
                    .map(|(id, time)| captured(id, time))
                    .collect();
                (workload, plan)
            })
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config())]

        #[test]
        fn deliveries_are_sorted_and_counted((workload, plan) in case()) {
            let schedule = apply_fault_plan(&workload, &plan).unwrap();
            prop_assert!(schedule.deliveries.windows(2).all(|pair| pair[0].time <= pair[1].time));

            let dropped: BTreeSet<EventId> = plan
                .iter()
                .filter_map(|op| match op {
                    FaultOp::Drop { event_id } => Some(*event_id),
                    _ => None,
                })
                .collect();
            let copies = plan
                .iter()
                .filter(|op| matches!(op, FaultOp::Duplicate { event_id } if !dropped.contains(event_id)))
                .count();
            prop_assert_eq!(schedule.deliveries.len(), workload.len() - dropped.len() + copies);
        }

        #[test]
        fn plan_order_only_matters_between_reorders(
            (workload, plan) in case(),
            shuffle_seed in any::<u32>(),
        ) {
            let plan: Vec<FaultOp> = plan
                .into_iter()
                .filter(|op| !matches!(op, FaultOp::Reorder { .. }))
                .collect();
            let mut permuted = plan.clone();
            Rng::from_seed(shuffle_seed).shuffle(&mut permuted);
            prop_assert_eq!(
                apply_fault_plan(&workload, &plan),
                apply_fault_plan(&workload, &permuted)
            );
        }
    }
}
