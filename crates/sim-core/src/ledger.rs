//! Double-entry ledger.
//!
//! `post()` enforces accounting *structure* only: balanced entries, declared
//! accounts, no overflow. It deliberately does NOT enforce business rules
//! (one capture per intent, refunds within capture): those are invariants
//! checked after the run, so a buggy handler's mistakes stay visible instead
//! of being swallowed as rejected posts.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::event::EventId;
use crate::money::Money;

/// The business operation an entry belongs to (e.g. one checkout). A newtype
/// so it can't be confused with an account name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntentId(pub String);

// Rail-agnostic by design: an ACH debit/return posts as Capture/Refund too
// (README's rail event types stay rail-specific; this is the abstracted
// accounting operation a handler derives from one). A card chargeback/
// dispute will need its own variant later — it's forced and can bypass
// normal refund-timing rules — but that's deferred until disputes are
// in scope; don't add it speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    Capture,
    Refund,
}

/// Signed change to one account: positive raises its balance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub account: String,
    pub delta: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub source: EventId,
    pub intent: IntentId,
    pub kind: EntryKind,
    pub postings: Vec<Posting>,
}

impl JournalEntry {
    /// Net of all deltas; zero for a balanced entry. `None` on overflow.
    pub fn net(&self) -> Option<Money> {
        Money::checked_sum(self.postings.iter().map(|p| p.delta))
    }

    /// Amount moved: the sum of the positive legs. Derived from the postings
    /// so there's no separate amount field that could disagree with them.
    pub fn gross(&self) -> Option<Money> {
        Money::checked_sum(
            self.postings
                .iter()
                .map(|p| p.delta)
                .filter(|delta| *delta > Money::ZERO),
        )
    }
}

/// Two-legged entry moving `amount` from `from` to `to`. `None` unless
/// `amount` is positive.
pub fn transfer(from: &str, to: &str, amount: Money) -> Option<Vec<Posting>> {
    if amount <= Money::ZERO {
        return None;
    }
    Some(vec![
        Posting {
            account: from.to_string(),
            delta: amount.checked_neg()?,
        },
        Posting {
            account: to.to_string(),
            delta: amount,
        },
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    DuplicateAccount(String),
    UnknownAccount(String),
    EmptyEntry,
    Unbalanced { sum: Money },
    Overflow,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::DuplicateAccount(account) => {
                write!(f, "account {account:?} is declared more than once")
            }
            LedgerError::UnknownAccount(account) => {
                write!(
                    f,
                    "account {account:?} was not declared when the ledger was opened"
                )
            }
            LedgerError::EmptyEntry => write!(f, "journal entry has no postings"),
            LedgerError::Unbalanced { sum } => write!(f, "postings sum to {sum}, not 0"),
            LedgerError::Overflow => write!(f, "amount overflows i64 cents"),
        }
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug, Clone)]
pub struct Ledger {
    /// Kept so invariants can replay the journal from the starting point.
    opening: BTreeMap<String, Money>,
    accounts: BTreeMap<String, Money>,
    journal: Vec<JournalEntry>,
}

impl Ledger {
    /// Takes `(account, cents)` pairs, the shape of `Scenario.initial_ledger`.
    /// Every account must be declared here, and opening balances must sum to
    /// zero (include an explicit counterparty such as `external:customer`).
    pub fn open(opening: &[(String, i64)]) -> Result<Ledger, LedgerError> {
        let mut accounts = BTreeMap::new();
        for (account, cents) in opening {
            if accounts.insert(account.clone(), Money(*cents)).is_some() {
                return Err(LedgerError::DuplicateAccount(account.clone()));
            }
        }

        let sum = Money::checked_sum(accounts.values().copied()).ok_or(LedgerError::Overflow)?;
        if sum != Money::ZERO {
            return Err(LedgerError::Unbalanced { sum });
        }

        Ok(Ledger {
            opening: accounts.clone(),
            accounts,
            journal: Vec::new(),
        })
    }

    /// Applies `entry` atomically: on `Err`, balances and journal are unchanged.
    pub fn post(&mut self, entry: JournalEntry) -> Result<(), LedgerError> {
        if entry.postings.is_empty() {
            return Err(LedgerError::EmptyEntry);
        }
        if let Some(unknown) = entry
            .postings
            .iter()
            .find(|p| !self.accounts.contains_key(&p.account))
        {
            return Err(LedgerError::UnknownAccount(unknown.account.clone()));
        }
        let sum = entry.net().ok_or(LedgerError::Overflow)?;
        if sum != Money::ZERO {
            return Err(LedgerError::Unbalanced { sum });
        }

        // Stage every new balance before touching `accounts`, so an overflow
        // on a later leg can't leave earlier legs applied. Staging by account
        // also nets out an account that appears twice in one entry.
        let mut staged: BTreeMap<&str, Money> = BTreeMap::new();
        for posting in &entry.postings {
            let current = match staged.get(posting.account.as_str()) {
                Some(balance) => *balance,
                None => self
                    .balance(&posting.account)
                    .ok_or_else(|| LedgerError::UnknownAccount(posting.account.clone()))?,
            };
            let next = current
                .checked_add(posting.delta)
                .ok_or(LedgerError::Overflow)?;
            staged.insert(&posting.account, next);
        }

        for (account, balance) in staged {
            if let Some(slot) = self.accounts.get_mut(account) {
                *slot = balance;
            }
        }
        debug_assert_eq!(
            Money::checked_sum(self.accounts.values().copied()),
            Some(Money::ZERO),
            "a balanced post must keep the ledger summing to zero"
        );
        self.journal.push(entry);
        Ok(())
    }

    pub fn balance(&self, account: &str) -> Option<Money> {
        self.accounts.get(account).copied()
    }

    pub fn balances(&self) -> &BTreeMap<String, Money> {
        &self.accounts
    }

    pub fn opening(&self) -> &BTreeMap<String, Money> {
        &self.opening
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn snapshot(&self) -> LedgerSnapshot {
        LedgerSnapshot {
            accounts: self.accounts.clone(),
        }
    }

    /// Bypasses `post()` so tests can prove the invariant backstops catch a
    /// corrupted ledger, a state the public API can't produce.
    #[cfg(test)]
    pub(crate) fn corrupt_balance(&mut self, account: &str, balance: Money) {
        self.accounts.insert(account.to_string(), balance);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerSnapshot {
    pub accounts: BTreeMap<String, Money>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ACCOUNTS: [&str; 4] = ["external:bank", "customer", "merchant", "fees"];

    fn open_ledger() -> Ledger {
        Ledger::open(&[
            ("external:bank".to_string(), -10_000),
            ("customer".to_string(), 10_000),
            ("merchant".to_string(), 0),
            ("fees".to_string(), 0),
        ])
        .unwrap()
    }

    fn entry(postings: Vec<Posting>) -> JournalEntry {
        JournalEntry {
            source: EventId(1),
            intent: IntentId("intent-1".to_string()),
            kind: EntryKind::Capture,
            postings,
        }
    }

    fn posting(account: &str, cents: i64) -> Posting {
        Posting {
            account: account.to_string(),
            delta: Money(cents),
        }
    }

    fn assert_rejected(ledger: &mut Ledger, rejected: JournalEntry, expected: LedgerError) {
        let before = ledger.clone();
        assert_eq!(ledger.post(rejected), Err(expected));
        assert_eq!(ledger.snapshot(), before.snapshot());
        assert_eq!(ledger.journal(), before.journal());
    }

    #[test]
    fn open_rejects_duplicate_accounts() {
        let opening = [("a".to_string(), 0), ("a".to_string(), 0)];
        assert_eq!(
            Ledger::open(&opening).unwrap_err(),
            LedgerError::DuplicateAccount("a".to_string())
        );
    }

    #[test]
    fn open_rejects_unbalanced_opening() {
        let opening = [("a".to_string(), 500), ("b".to_string(), 0)];
        assert_eq!(
            Ledger::open(&opening).unwrap_err(),
            LedgerError::Unbalanced { sum: Money(500) }
        );
    }

    #[test]
    fn post_applies_transfer_and_records_it() {
        let mut ledger = open_ledger();
        let capture = entry(transfer("customer", "merchant", Money(2_500)).unwrap());
        ledger.post(capture.clone()).unwrap();

        assert_eq!(ledger.balance("customer"), Some(Money(7_500)));
        assert_eq!(ledger.balance("merchant"), Some(Money(2_500)));
        assert_eq!(ledger.journal(), [capture]);
    }

    #[test]
    fn post_nets_an_account_repeated_within_one_entry() {
        let mut ledger = open_ledger();
        let split = entry(vec![
            posting("customer", -100),
            posting("customer", -50),
            posting("merchant", 150),
        ]);
        ledger.post(split).unwrap();

        assert_eq!(ledger.balance("customer"), Some(Money(9_850)));
        assert_eq!(ledger.balance("merchant"), Some(Money(150)));
    }

    #[test]
    fn post_rejects_empty_entry() {
        let mut ledger = open_ledger();
        assert_rejected(&mut ledger, entry(vec![]), LedgerError::EmptyEntry);
    }

    #[test]
    fn post_rejects_undeclared_account() {
        let mut ledger = open_ledger();
        let typo = entry(transfer("customer", "merhcant", Money(100)).unwrap());
        assert_rejected(
            &mut ledger,
            typo,
            LedgerError::UnknownAccount("merhcant".to_string()),
        );
    }

    #[test]
    fn post_rejects_unbalanced_entry() {
        let mut ledger = open_ledger();
        let unbalanced = entry(vec![posting("customer", -100), posting("merchant", 90)]);
        assert_rejected(
            &mut ledger,
            unbalanced,
            LedgerError::Unbalanced { sum: Money(-10) },
        );
    }

    #[test]
    fn post_rejects_overflowing_net() {
        let mut ledger = open_ledger();
        let overflow = entry(vec![posting("customer", i64::MAX), posting("merchant", 1)]);
        assert_rejected(&mut ledger, overflow, LedgerError::Overflow);
    }

    #[test]
    fn post_rejects_balance_overflow_without_applying_earlier_legs() {
        let mut ledger = Ledger::open(&[
            ("big".to_string(), i64::MAX),
            ("sink".to_string(), -i64::MAX),
        ])
        .unwrap();
        // The `sink` leg is valid on its own; it must not be applied when the
        // `big` leg overflows.
        let overflow = entry(vec![posting("sink", -1), posting("big", 1)]);
        assert_rejected(&mut ledger, overflow, LedgerError::Overflow);
    }

    #[test]
    fn transfer_requires_positive_amount() {
        assert_eq!(transfer("a", "b", Money::ZERO), None);
        assert_eq!(transfer("a", "b", Money(-1)), None);
        assert_eq!(
            transfer("a", "b", Money(1)),
            Some(vec![posting("a", -1), posting("b", 1)])
        );
    }

    #[test]
    fn gross_sums_positive_legs() {
        let split = entry(vec![
            posting("customer", -150),
            posting("merchant", 140),
            posting("fees", 10),
        ]);
        assert_eq!(split.gross(), Some(Money(150)));
        assert_eq!(split.net(), Some(Money::ZERO));
    }

    /// Up to 5 random legs plus a final leg that cancels their sum.
    fn balanced_postings() -> impl Strategy<Value = Vec<Posting>> {
        let leg = (0..ACCOUNTS.len(), -1_000_000_000i64..1_000_000_000);
        (prop::collection::vec(leg, 1..=5), 0..ACCOUNTS.len()).prop_map(|(legs, last)| {
            let mut postings: Vec<Posting> = legs
                .into_iter()
                .map(|(account, cents)| posting(ACCOUNTS[account], cents))
                .collect();
            let net: i64 = postings.iter().map(|p| p.delta.0).sum();
            postings.push(posting(ACCOUNTS[last], -net));
            postings
        })
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config())]

        #[test]
        fn balanced_posts_keep_ledger_at_zero_and_explained_by_journal(
            entries in prop::collection::vec(balanced_postings(), 1..20)
        ) {
            let mut ledger = open_ledger();
            for postings in entries {
                ledger.post(entry(postings)).unwrap();
                prop_assert_eq!(
                    Money::checked_sum(ledger.balances().values().copied()),
                    Some(Money::ZERO)
                );
            }

            let mut replayed = ledger.opening().clone();
            for posting in ledger.journal().iter().flat_map(|e| &e.postings) {
                let balance = replayed.get_mut(&posting.account).unwrap();
                *balance = balance.checked_add(posting.delta).unwrap();
            }
            prop_assert_eq!(&replayed, ledger.balances());
        }

        #[test]
        fn nudged_entry_is_rejected_and_ledger_unchanged(
            postings in balanced_postings(),
            leg in any::<prop::sample::Index>(),
            nudge in prop_oneof![-1_000_000i64..0, 1..1_000_000i64],
        ) {
            let mut postings = postings;
            let leg = leg.index(postings.len());
            postings[leg].delta = Money(postings[leg].delta.0 + nudge);

            let mut ledger = open_ledger();
            let before = ledger.clone();
            prop_assert_eq!(
                ledger.post(entry(postings)),
                Err(LedgerError::Unbalanced { sum: Money(nudge) })
            );
            prop_assert_eq!(ledger.snapshot(), before.snapshot());
            prop_assert!(ledger.journal().is_empty());
        }
    }
}
