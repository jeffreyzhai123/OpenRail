use serde::{Deserialize, Serialize};
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::rails::ach::AchEvent;
use crate::rails::card::CardEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

// Thin dispatcher over per-rail event vocabularies (crate::rails::{ach,card}).
// Each rail owns its own fields and state machine; adding a rail (e.g. RTP)
// is an additive variant here, not a merge into an unrelated field list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Card(CardEvent),
    Ach(AchEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimEvent {
    pub id: EventId,
    pub time: u64,
    pub seq: u64,
    pub kind: EventKind,
}

// Eq (derived) compares every field; Ord compares (time, seq) only, so the
// two can disagree in general. Safe here because EventQueue::push assigns
// seq uniquely per queue (strictly increasing), so two distinct elements of
// one queue never tie under this Ord — proved by EventQueue's "pops are
// strictly increasing in (time, seq)" test (deterministic-engine-plan.md,
// E3). Don't use this Ord outside EventQueue.
impl Ord for SimEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.time, self.seq).cmp(&(other.time, other.seq))
    }
}

impl PartialOrd for SimEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The queue is the sole assigner of `seq`, so `push` takes the parts, not a
/// `SimEvent` — a caller can't supply one that collides or is out of order.
#[derive(Default)]
pub struct EventQueue {
    heap: BinaryHeap<Reverse<SimEvent>>,
    next_seq: u64,
}

impl EventQueue {
    pub fn push(&mut self, id: EventId, time: u64, kind: EventKind) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.heap.push(Reverse(SimEvent {
            id,
            time,
            seq,
            kind,
        }));
    }

    pub fn pop(&mut self) -> Option<SimEvent> {
        self.heap.pop().map(|Reverse(event)| event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Money;
    use crate::rails::card::{CardEvent, ChargeId};
    use proptest::prelude::*;

    fn kind() -> EventKind {
        EventKind::Card(CardEvent::Captured {
            charge_id: ChargeId(0),
            amount: Money(100),
        })
    }

    #[test]
    fn same_time_events_pop_in_push_order() {
        let mut q = EventQueue::default();
        q.push(EventId(1), 100, kind());
        q.push(EventId(2), 100, kind());
        assert_eq!(q.pop().map(|e| e.id), Some(EventId(1)));
        assert_eq!(q.pop().map(|e| e.id), Some(EventId(2)));
    }

    #[test]
    fn earlier_time_pushed_later_still_pops_first() {
        let mut q = EventQueue::default();
        q.push(EventId(1), 200, kind());
        q.push(EventId(2), 100, kind());
        assert_eq!(q.pop().map(|e| e.id), Some(EventId(2)));
        assert_eq!(q.pop().map(|e| e.id), Some(EventId(1)));
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config())]

        #[test]
        fn pops_are_strictly_increasing_and_counted(times in prop::collection::vec(0u64..10, 1..50)) {
            let mut q = EventQueue::default();
            for (i, t) in times.iter().enumerate() {
                q.push(EventId(i as u64), *t, kind());
            }
            let mut last: Option<(u64, u64)> = None;
            let mut count = 0;
            while let Some(e) = q.pop() {
                count += 1;
                let key = (e.time, e.seq);
                if let Some(prev) = last {
                    prop_assert!(prev < key);
                }
                last = Some(key);
            }
            prop_assert_eq!(count, times.len());
        }
    }
}
