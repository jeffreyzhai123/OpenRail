//! A run's fingerprint. A replay recomputes it and shows "verified identical"
//! only when it matches (README §6.2).

use crate::event::SimEvent;
use crate::ledger::JournalEntry;

/// blake3 of `serde_json::to_vec(&(trace, journal))`, as 64 lowercase hex
/// chars. It covers the journal as well as the events, so a matching hash
/// means the same money movements too, not just the same deliveries.
///
/// Plain serde_json is canonical here: derived `Serialize` writes fields in
/// declaration order, and the hashed types have no maps and no floats. Any
/// map added to them must be a `BTreeMap`. Renaming or reordering a field
/// changes every hash, which needs a replay-encoding version bump.
pub fn hash_run(trace: &[SimEvent], journal: &[JournalEntry]) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(&(trace, journal))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventId, EventKind};
    use crate::ledger::{EntryKind, IntentId, transfer};
    use crate::money::Money;
    use crate::rails::ach::{AchEntryId, AchEvent, AchReturnCode};
    use crate::rails::card::{CardEvent, ChargeId};

    fn event(id: u64, time: u64, seq: u64, kind: EventKind) -> SimEvent {
        SimEvent {
            id: EventId(id),
            time,
            seq,
            kind,
        }
    }

    fn entry(source: u64, kind: EntryKind, from: &str, to: &str, cents: i64) -> JournalEntry {
        JournalEntry {
            source: EventId(source),
            intent: IntentId("charge-1".to_string()),
            kind,
            postings: transfer(from, to, Money(cents)).unwrap(),
        }
    }

    /// A capture, a partial refund of it, and an unrelated ACH return.
    fn fixture() -> (Vec<SimEvent>, Vec<JournalEntry>) {
        let trace = vec![
            event(
                1,
                1_000,
                0,
                EventKind::Card(CardEvent::Captured {
                    charge_id: ChargeId(1),
                    amount: Money(2_500),
                }),
            ),
            event(
                2,
                2_000,
                1,
                EventKind::Card(CardEvent::Refunded {
                    charge_id: ChargeId(1),
                    amount: Money(500),
                }),
            ),
            event(
                3,
                3_000,
                2,
                EventKind::Ach(AchEvent::Returned {
                    entry_id: AchEntryId(7),
                    code: AchReturnCode::R01,
                    amount: Money(1_200),
                }),
            ),
        ];
        let journal = vec![
            entry(1, EntryKind::Capture, "external:card", "merchant", 2_500),
            entry(2, EntryKind::Refund, "merchant", "external:card", 500),
        ];
        (trace, journal)
    }

    fn fixture_hash() -> String {
        let (trace, journal) = fixture();
        hash_run(&trace, &journal).unwrap()
    }

    /// Asserts that `mutate` changes the fixture's hash.
    fn assert_changes_hash(mutate: impl FnOnce(&mut Vec<SimEvent>, &mut Vec<JournalEntry>)) {
        let (mut trace, mut journal) = fixture();
        mutate(&mut trace, &mut journal);
        assert_ne!(hash_run(&trace, &journal).unwrap(), fixture_hash());
    }

    fn set_card_amount(event: &mut SimEvent, cents: i64) {
        if let EventKind::Card(
            CardEvent::Captured { amount, .. } | CardEvent::Refunded { amount, .. },
        ) = &mut event.kind
        {
            *amount = Money(cents);
        }
    }

    #[test]
    fn same_input_gives_the_same_lowercase_hex_hash() {
        let hash = fixture_hash();
        assert_eq!(hash, fixture_hash());
        assert_eq!(hash.len(), 64);
        assert!(
            hash.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn golden_hash_is_pinned() {
        // If this fails, the hashed bytes changed: every existing replay link
        // stops verifying. Bump the replay-encoding version (README §6.2)
        // before updating this value.
        assert_eq!(
            fixture_hash(),
            "6f89d7fd8ccc2ded581d3cae80a425158914636d6db0ba98b6d87bc4fa11000c"
        );
    }

    #[test]
    fn empty_run_hashes() {
        let hash = hash_run(&[], &[]).unwrap();
        assert_eq!(hash.len(), 64);
        assert_ne!(hash, fixture_hash());
    }

    #[test]
    fn any_event_change_changes_the_hash() {
        assert_changes_hash(|trace, _| trace[0].time += 1);
        assert_changes_hash(|trace, _| trace[0].seq += 10);
        assert_changes_hash(|trace, _| trace[0].id = EventId(99));
        assert_changes_hash(|trace, _| set_card_amount(&mut trace[1], 501));
        assert_changes_hash(|trace, _| trace.swap(0, 1));
    }

    #[test]
    fn any_journal_change_changes_the_hash() {
        assert_changes_hash(|_, journal| {
            journal[1] = entry(2, EntryKind::Refund, "merchant", "external:card", 501);
        });
        assert_changes_hash(|_, journal| {
            journal.pop();
        });
    }
}
