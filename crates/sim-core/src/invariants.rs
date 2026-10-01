//! The six ledger invariants (README §6.4), checked once at the end of a run
//! over the whole journal, so "always"/"never" rules still see history.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ledger::{EntryKind, IntentId, Ledger};
use crate::money::Money;

// Stable identities: the shrinker matches on these and they end up in replay
// links. Never rename one; add a new name instead.
pub const LEDGER_BALANCED: &str = "ledger_balanced";
pub const MONEY_CONSERVED: &str = "money_conserved";
pub const SINGLE_CAPTURE_PER_INTENT: &str = "single_capture_per_intent";
pub const REFUND_WITHIN_CAPTURE: &str = "refund_within_capture";
// #5 and #6 need provider events, which don't exist yet. They are named here
// but not run: a stub that always passes would be a fake result.
pub const RECONCILES_WITH_PROVIDER: &str = "reconciles_with_provider";
pub const ENTRY_HAS_PROVIDER_EVENT: &str = "entry_has_provider_event";

// The shrinker's stopping condition is "the *same named* invariant still
// fails" (README §6.1), so a name/identity is required, not just pass/fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantResult {
    pub name: &'static str,
    pub passed: bool,
    pub message: Option<String>,
}

impl InvariantResult {
    fn pass(name: &'static str) -> Self {
        InvariantResult {
            name,
            passed: true,
            message: None,
        }
    }

    fn fail(name: &'static str, message: String) -> Self {
        InvariantResult {
            name,
            passed: false,
            message: Some(message),
        }
    }
}

/// Everything a check may look at. A struct rather than a bare `&Ledger` so
/// later checks (provider state, trace) add fields without changing callers.
pub struct InvariantContext<'a> {
    pub ledger: &'a Ledger,
}

pub trait InvariantCheck {
    fn name(&self) -> &'static str;
    fn check(&self, ctx: &InvariantContext<'_>) -> InvariantResult;
}

/// Fixed order, because `RunResult.invariants` is serialized and hashed.
const CHECKS: [&dyn InvariantCheck; 4] = [
    &LedgerBalanced,
    &MoneyConserved,
    &SingleCapturePerIntent,
    &RefundWithinCapture,
];

pub fn check_all(ctx: &InvariantContext<'_>) -> Vec<InvariantResult> {
    CHECKS.iter().map(|check| check.check(ctx)).collect()
}

/// #1: every journal entry nets to zero, and so do current balances.
/// Guaranteed by `Ledger::post` in V1; this is the backstop.
pub struct LedgerBalanced;

impl InvariantCheck for LedgerBalanced {
    fn name(&self) -> &'static str {
        LEDGER_BALANCED
    }

    fn check(&self, ctx: &InvariantContext<'_>) -> InvariantResult {
        for (index, entry) in ctx.ledger.journal().iter().enumerate() {
            match entry.net() {
                Some(Money::ZERO) => {}
                Some(net) => {
                    return InvariantResult::fail(
                        self.name(),
                        format!(
                            "entry #{index} (event {}) nets to {net}, not 0",
                            entry.source.0
                        ),
                    );
                }
                None => {
                    return InvariantResult::fail(
                        self.name(),
                        format!(
                            "entry #{index} (event {}) overflows when summed",
                            entry.source.0
                        ),
                    );
                }
            }
        }

        match Money::checked_sum(ctx.ledger.balances().values().copied()) {
            Some(Money::ZERO) => InvariantResult::pass(self.name()),
            Some(total) => {
                InvariantResult::fail(self.name(), format!("balances sum to {total}, not 0"))
            }
            None => InvariantResult::fail(self.name(), "balances overflow when summed".to_string()),
        }
    }
}

/// #2: replaying the journal over the opening balances reproduces the current
/// balances exactly, i.e. no money moved without a recorded entry.
/// Guaranteed by `Ledger::post` in V1; load-bearing once V3 shards the ledger.
pub struct MoneyConserved;

impl InvariantCheck for MoneyConserved {
    fn name(&self) -> &'static str {
        MONEY_CONSERVED
    }

    fn check(&self, ctx: &InvariantContext<'_>) -> InvariantResult {
        let mut replayed = ctx.ledger.opening().clone();
        for (index, entry) in ctx.ledger.journal().iter().enumerate() {
            for posting in &entry.postings {
                let Some(balance) = replayed.get_mut(&posting.account) else {
                    return InvariantResult::fail(
                        self.name(),
                        format!(
                            "entry #{index} posts to undeclared account {:?}",
                            posting.account
                        ),
                    );
                };
                let Some(next) = balance.checked_add(posting.delta) else {
                    return InvariantResult::fail(
                        self.name(),
                        format!("replaying entry #{index} overflows {:?}", posting.account),
                    );
                };
                *balance = next;
            }
        }

        let actual = ctx.ledger.balances();
        for (account, balance) in actual {
            let explained = replayed.get(account).copied().unwrap_or(Money::ZERO);
            if *balance != explained {
                return InvariantResult::fail(
                    self.name(),
                    format!("{account:?} holds {balance} but the journal explains {explained}"),
                );
            }
        }
        if let Some(missing) = replayed
            .keys()
            .find(|account| !actual.contains_key(*account))
        {
            return InvariantResult::fail(
                self.name(),
                format!("{missing:?} is in the journal but missing from balances"),
            );
        }
        InvariantResult::pass(self.name())
    }
}

/// #3: at most one capture per business intent (idempotency).
pub struct SingleCapturePerIntent;

impl InvariantCheck for SingleCapturePerIntent {
    fn name(&self) -> &'static str {
        SINGLE_CAPTURE_PER_INTENT
    }

    fn check(&self, ctx: &InvariantContext<'_>) -> InvariantResult {
        let mut first_capture: BTreeMap<&IntentId, usize> = BTreeMap::new();
        for (index, entry) in ctx.ledger.journal().iter().enumerate() {
            if entry.kind != EntryKind::Capture {
                continue;
            }
            if let Some(first) = first_capture.insert(&entry.intent, index) {
                return InvariantResult::fail(
                    self.name(),
                    format!(
                        "intent {:?} captured again by entry #{index} (event {}); first capture was entry #{first}",
                        entry.intent.0, entry.source.0
                    ),
                );
            }
        }
        InvariantResult::pass(self.name())
    }
}

/// #4: at every point in the journal, an intent's refunds never exceed what
/// it has captured *so far*. Checking only final totals would miss a refund
/// that lands before its capture (scenario 2).
pub struct RefundWithinCapture;

impl InvariantCheck for RefundWithinCapture {
    fn name(&self) -> &'static str {
        REFUND_WITHIN_CAPTURE
    }

    fn check(&self, ctx: &InvariantContext<'_>) -> InvariantResult {
        // intent -> (captured, refunded)
        let mut totals: BTreeMap<&IntentId, (Money, Money)> = BTreeMap::new();
        for (index, entry) in ctx.ledger.journal().iter().enumerate() {
            let intent_totals = totals
                .entry(&entry.intent)
                .or_insert((Money::ZERO, Money::ZERO));
            let running = match entry.kind {
                EntryKind::Capture => &mut intent_totals.0,
                EntryKind::Refund => &mut intent_totals.1,
            };
            let Some(next) = entry.gross().and_then(|amount| running.checked_add(amount)) else {
                return InvariantResult::fail(
                    self.name(),
                    format!(
                        "totals for intent {:?} overflow at entry #{index}",
                        entry.intent.0
                    ),
                );
            };
            *running = next;

            let (captured, refunded) = *intent_totals;
            if refunded > captured {
                return InvariantResult::fail(
                    self.name(),
                    format!(
                        "intent {:?} refunded {refunded} with only {captured} captured, at entry #{index} (event {})",
                        entry.intent.0, entry.source.0
                    ),
                );
            }
        }
        InvariantResult::pass(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventId;
    use crate::ledger::{JournalEntry, transfer};

    const CARD: &str = "external:card";
    const MERCHANT: &str = "merchant";

    fn open_ledger() -> Ledger {
        Ledger::open(&[(CARD.to_string(), 0), (MERCHANT.to_string(), 0)]).unwrap()
    }

    fn capture(ledger: &mut Ledger, event: u64, intent: &str, cents: i64) {
        post(
            ledger,
            event,
            intent,
            EntryKind::Capture,
            CARD,
            MERCHANT,
            cents,
        );
    }

    fn refund(ledger: &mut Ledger, event: u64, intent: &str, cents: i64) {
        post(
            ledger,
            event,
            intent,
            EntryKind::Refund,
            MERCHANT,
            CARD,
            cents,
        );
    }

    fn post(
        ledger: &mut Ledger,
        event: u64,
        intent: &str,
        kind: EntryKind,
        from: &str,
        to: &str,
        cents: i64,
    ) {
        ledger
            .post(JournalEntry {
                source: EventId(event),
                intent: IntentId(intent.to_string()),
                kind,
                postings: transfer(from, to, Money(cents)).unwrap(),
            })
            .unwrap();
    }

    fn passed(ledger: &Ledger, name: &str) -> bool {
        check_all(&InvariantContext { ledger })
            .into_iter()
            .find(|result| result.name == name)
            .unwrap()
            .passed
    }

    #[test]
    fn check_all_reports_named_checks_in_fixed_order() {
        let ledger = open_ledger();
        let names: Vec<_> = check_all(&InvariantContext { ledger: &ledger })
            .iter()
            .map(|result| result.name)
            .collect();
        assert_eq!(
            names,
            [
                LEDGER_BALANCED,
                MONEY_CONSERVED,
                SINGLE_CAPTURE_PER_INTENT,
                REFUND_WITHIN_CAPTURE
            ]
        );
    }

    #[test]
    fn clean_capture_and_partial_refund_pass_everything() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        refund(&mut ledger, 2, "order-1", 2_000);

        let results = check_all(&InvariantContext { ledger: &ledger });
        assert!(results.iter().all(|r| r.passed), "{results:?}");
    }

    #[test]
    fn corrupted_balance_fails_balanced_and_conserved() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        ledger.corrupt_balance(MERCHANT, Money(9_999));

        assert!(!passed(&ledger, LEDGER_BALANCED));
        assert!(!passed(&ledger, MONEY_CONSERVED));
    }

    #[test]
    fn balanced_but_unexplained_balance_fails_money_conserved() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        // Still sums to zero, but not what the journal says happened.
        ledger.corrupt_balance(CARD, Money(-6_000));
        ledger.corrupt_balance(MERCHANT, Money(6_000));

        assert!(passed(&ledger, LEDGER_BALANCED));
        assert!(!passed(&ledger, MONEY_CONSERVED));
    }

    #[test]
    fn duplicate_capture_fails_only_single_capture() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        capture(&mut ledger, 2, "order-1", 5_000);

        for result in check_all(&InvariantContext { ledger: &ledger }) {
            assert_eq!(
                result.passed,
                result.name != SINGLE_CAPTURE_PER_INTENT,
                "{result:?}"
            );
        }
    }

    #[test]
    fn captures_for_different_intents_pass() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        capture(&mut ledger, 2, "order-2", 5_000);

        assert!(passed(&ledger, SINGLE_CAPTURE_PER_INTENT));
    }

    #[test]
    fn over_refund_fails() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        refund(&mut ledger, 2, "order-1", 3_000);
        refund(&mut ledger, 3, "order-1", 3_000);

        assert!(!passed(&ledger, REFUND_WITHIN_CAPTURE));
    }

    #[test]
    fn refund_before_capture_fails_even_when_final_totals_match() {
        let mut ledger = open_ledger();
        refund(&mut ledger, 1, "order-1", 5_000);
        capture(&mut ledger, 2, "order-1", 5_000);

        assert!(!passed(&ledger, REFUND_WITHIN_CAPTURE));
    }

    #[test]
    fn full_refund_passes() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        refund(&mut ledger, 2, "order-1", 5_000);

        assert!(passed(&ledger, REFUND_WITHIN_CAPTURE));
    }

    #[test]
    fn refunds_are_tracked_per_intent() {
        let mut ledger = open_ledger();
        capture(&mut ledger, 1, "order-1", 5_000);
        capture(&mut ledger, 2, "order-2", 1_000);
        // order-2 has only 1_000 captured, even though the merchant holds 6_000.
        refund(&mut ledger, 3, "order-2", 2_000);

        assert!(!passed(&ledger, REFUND_WITHIN_CAPTURE));
    }
}
