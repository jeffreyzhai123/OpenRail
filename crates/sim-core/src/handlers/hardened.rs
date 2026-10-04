use crate::event::{EventKind, SimEvent};
use crate::ledger::{JournalEntry, Ledger};
use crate::rails::ach::AchEvent;
use crate::rails::card::CardEvent;

use super::{
    EventHandler, ach_capture_entry, ach_refund_entry, already_captured, already_posted,
    capture_entry, card_intent, refund_entry,
};

/// Everything checked against `ledger.journal()`, never memory — nothing to
/// lose on a `CrashRestart`.
pub struct HardenedHandler;

impl EventHandler for HardenedHandler {
    fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry> {
        if already_posted(ledger, event.id) {
            return vec![]; // redelivered webhook: already posted
        }
        let entry = match &event.kind {
            EventKind::Card(CardEvent::Captured { charge_id, amount }) => {
                if already_captured(ledger, &card_intent(*charge_id)) {
                    None // retried capture, different event id, same intent
                } else {
                    capture_entry(event.id, *charge_id, *amount)
                }
            }
            EventKind::Card(CardEvent::Refunded { charge_id, amount }) => {
                if already_captured(ledger, &card_intent(*charge_id)) {
                    refund_entry(event.id, *charge_id, *amount)
                } else {
                    None // D4 (temporary, decisions-log.md): reject a premature refund
                }
            }
            EventKind::Ach(AchEvent::Initiated { entry_id, amount }) => {
                ach_capture_entry(event.id, *entry_id, *amount)
            }
            EventKind::Ach(AchEvent::Returned {
                entry_id, amount, ..
            }) => ach_refund_entry(event.id, *entry_id, *amount),
            _ => None,
        };
        entry.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventId;
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

    fn event(id: u64, kind: EventKind) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time: 0,
            seq: 0,
            kind,
        }
    }

    fn captured(id: u64, charge: u64, cents: i64) -> SimEvent {
        event(
            id,
            EventKind::Card(CardEvent::Captured {
                charge_id: ChargeId(charge),
                amount: Money(cents),
            }),
        )
    }

    fn post(ledger: &mut Ledger, handler: &mut HardenedHandler, event: &SimEvent) -> usize {
        let entries = handler.handle(event, ledger);
        let count = entries.len();
        for entry in entries {
            ledger.post(entry).unwrap();
        }
        count
    }

    #[test]
    fn redelivered_capture_posts_only_once() {
        let mut handler = HardenedHandler;
        let mut ledger = ledger();
        let capture = captured(1, 1, 500);
        assert_eq!(post(&mut ledger, &mut handler, &capture), 1);
        assert_eq!(post(&mut ledger, &mut handler, &capture), 0); // same event id
    }

    #[test]
    fn retried_capture_with_a_new_event_id_posts_only_once() {
        let mut handler = HardenedHandler;
        let mut ledger = ledger();
        assert_eq!(post(&mut ledger, &mut handler, &captured(1, 1, 500)), 1);
        // Different event id, same charge/intent: a client retry, not a
        // provider redelivery, but still only one capture.
        assert_eq!(post(&mut ledger, &mut handler, &captured(2, 1, 500)), 0);
    }

    #[test]
    fn refund_before_capture_is_rejected() {
        let mut handler = HardenedHandler;
        let mut ledger = ledger();
        let refund = event(
            1,
            EventKind::Card(CardEvent::Refunded {
                charge_id: ChargeId(1),
                amount: Money(200),
            }),
        );
        assert_eq!(post(&mut ledger, &mut handler, &refund), 0);
    }

    #[test]
    fn refund_after_its_capture_posts() {
        let mut handler = HardenedHandler;
        let mut ledger = ledger();
        post(&mut ledger, &mut handler, &captured(1, 1, 500));
        let refund = event(
            2,
            EventKind::Card(CardEvent::Refunded {
                charge_id: ChargeId(1),
                amount: Money(200),
            }),
        );
        assert_eq!(post(&mut ledger, &mut handler, &refund), 1);
    }

    #[test]
    fn crash_restart_loses_no_correctness_since_state_lives_in_the_journal() {
        // A fresh handler instance (as a CrashRestart gives) still sees the
        // prior capture, because it reads the journal, not memory.
        let mut ledger = ledger();
        post(&mut ledger, &mut HardenedHandler, &captured(1, 1, 500));
        let mut restarted = HardenedHandler;
        assert_eq!(post(&mut ledger, &mut restarted, &captured(1, 1, 500)), 0);
    }
}
