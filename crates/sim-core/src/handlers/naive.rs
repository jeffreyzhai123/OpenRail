use crate::event::{EventKind, SimEvent};
use crate::ledger::{JournalEntry, Ledger};
use crate::rails::ach::AchEvent;
use crate::rails::card::CardEvent;

use super::{EventHandler, ach_capture_entry, ach_refund_entry, capture_entry, refund_entry};

/// Posts unconditionally: no dedup, no ordering check. Exactly what
/// invariants #3/#4/#7 exist to catch under a Duplicate/Reorder fault.
pub struct NaiveHandler;

impl EventHandler for NaiveHandler {
    fn handle(&mut self, event: &SimEvent, _ledger: &Ledger) -> Vec<JournalEntry> {
        let entry = match &event.kind {
            EventKind::Card(CardEvent::Captured { charge_id, amount }) => {
                capture_entry(event.id, *charge_id, *amount)
            }
            EventKind::Card(CardEvent::Refunded { charge_id, amount }) => {
                refund_entry(event.id, *charge_id, *amount)
            }
            EventKind::Ach(AchEvent::Initiated { entry_id, amount }) => {
                ach_capture_entry(event.id, *entry_id, *amount)
            }
            EventKind::Ach(AchEvent::Returned {
                entry_id, amount, ..
            }) => ach_refund_entry(event.id, *entry_id, *amount),
            // Authorized holds funds but doesn't move them; Batched/Settled
            // move no money (rails/ach.rs).
            _ => None,
        };
        entry.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventId;
    use crate::ledger::EntryKind;
    use crate::money::Money;
    use crate::rails::card::ChargeId;

    fn ledger() -> Ledger {
        Ledger::open(&[
            ("external:card".to_string(), 0),
            ("external:bank".to_string(), 0),
            ("merchant".to_string(), 0),
        ])
        .unwrap()
    }

    fn captured(id: u64, charge: u64, cents: i64) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time: 0,
            seq: 0,
            kind: EventKind::Card(CardEvent::Captured {
                charge_id: ChargeId(charge),
                amount: Money(cents),
            }),
        }
    }

    #[test]
    fn posts_every_capture_even_a_redelivered_one() {
        let mut handler = NaiveHandler;
        let ledger = ledger();
        let entries_1 = handler.handle(&captured(1, 1, 500), &ledger);
        let entries_2 = handler.handle(&captured(1, 1, 500), &ledger); // same event id
        assert_eq!(entries_1.len(), 1);
        assert_eq!(entries_2.len(), 1);
        assert_eq!(entries_1[0].kind, EntryKind::Capture);
    }

    #[test]
    fn authorized_posts_nothing() {
        let mut handler = NaiveHandler;
        let event = SimEvent {
            id: EventId(1),
            time: 0,
            seq: 0,
            kind: EventKind::Card(CardEvent::Authorized {
                charge_id: ChargeId(1),
                amount: Money(500),
            }),
        };
        assert_eq!(handler.handle(&event, &ledger()), vec![]);
    }
}
