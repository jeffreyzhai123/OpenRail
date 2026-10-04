pub mod hardened;
pub mod naive;

use serde::{Deserialize, Serialize};

use crate::event::{EventId, SimEvent};
use crate::ledger::{EntryKind, IntentId, JournalEntry, Ledger, transfer};
use crate::money::Money;
use crate::rails::ach::AchEntryId;
use crate::rails::card::ChargeId;

/// Returns entries instead of mutating the ledger, so `run()` stays the only
/// caller of `post()`. `&mut self` for handler-local memory; `&Ledger` is
/// read-only so a handler can derive durable idempotency from `journal()`.
pub trait EventHandler {
    fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry>;
}

const EXTERNAL_CARD: &str = "external:card";
const EXTERNAL_BANK: &str = "external:bank";
const MERCHANT: &str = "merchant";

/// Shared by both handlers so a card's capture and its refund always key to
/// the same intent, whichever posts first.
fn card_intent(charge_id: ChargeId) -> IntentId {
    IntentId(format!("charge-{}", charge_id.0))
}

fn ach_intent(entry_id: AchEntryId) -> IntentId {
    IntentId(format!("ach-{}", entry_id.0))
}

/// `None` iff `amount` isn't positive — `transfer()`'s contract, not an
/// error: a handler has no `Result` to report through (the trait has none),
/// so a non-positive amount simply posts nothing.
fn capture_entry(source: EventId, charge_id: ChargeId, amount: Money) -> Option<JournalEntry> {
    Some(JournalEntry {
        source,
        intent: card_intent(charge_id),
        kind: EntryKind::Capture,
        postings: transfer(EXTERNAL_CARD, MERCHANT, amount)?,
    })
}

fn refund_entry(source: EventId, charge_id: ChargeId, amount: Money) -> Option<JournalEntry> {
    Some(JournalEntry {
        source,
        intent: card_intent(charge_id),
        kind: EntryKind::Refund,
        postings: transfer(MERCHANT, EXTERNAL_CARD, amount)?,
    })
}

fn ach_capture_entry(source: EventId, entry_id: AchEntryId, amount: Money) -> Option<JournalEntry> {
    Some(JournalEntry {
        source,
        intent: ach_intent(entry_id),
        kind: EntryKind::Capture,
        postings: transfer(EXTERNAL_BANK, MERCHANT, amount)?,
    })
}

fn ach_refund_entry(source: EventId, entry_id: AchEntryId, amount: Money) -> Option<JournalEntry> {
    Some(JournalEntry {
        source,
        intent: ach_intent(entry_id),
        kind: EntryKind::Refund,
        postings: transfer(MERCHANT, EXTERNAL_BANK, amount)?,
    })
}

/// A redelivered webhook: some earlier entry already came from this event.
fn already_posted(ledger: &Ledger, source: EventId) -> bool {
    ledger.journal().iter().any(|entry| entry.source == source)
}

/// A capture already exists for this intent (used for the retry and
/// refund-ordering checks, which key on intent rather than event id).
fn already_captured(ledger: &Ledger, intent: &IntentId) -> bool {
    ledger
        .journal()
        .iter()
        .any(|entry| entry.intent == *intent && entry.kind == EntryKind::Capture)
}

/// A name only — not composite like `EventKind`. `build()` lands once
/// `naive.rs`/`hardened.rs` have types to construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandlerKind {
    Naive,
    Hardened,
}

impl HandlerKind {
    pub fn build(self) -> Box<dyn EventHandler> {
        match self {
            HandlerKind::Naive => Box::new(naive::NaiveHandler),
            HandlerKind::Hardened => Box::new(hardened::HardenedHandler),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&HandlerKind::Naive).unwrap(),
            "\"naive\""
        );
        assert_eq!(
            serde_json::to_string(&HandlerKind::Hardened).unwrap(),
            "\"hardened\""
        );
    }

    #[test]
    fn unknown_name_fails_to_deserialize() {
        assert!(serde_json::from_str::<HandlerKind>("\"bogus\"").is_err());
    }
}
