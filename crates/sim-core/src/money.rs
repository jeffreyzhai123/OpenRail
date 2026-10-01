use std::fmt;

use serde::{Deserialize, Serialize};

const CENTS_PER_DOLLAR: u64 = 100;

/// Integer cents. Deliberately has no `Add`/`Sub`/`Neg` impls and no
/// `From<f64>`: every arithmetic path is checked, so overflow must be
/// handled at the call site instead of panicking or wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub fn checked_add(self, rhs: Money) -> Option<Money> {
        self.0.checked_add(rhs.0).map(Money)
    }

    pub fn checked_sub(self, rhs: Money) -> Option<Money> {
        self.0.checked_sub(rhs.0).map(Money)
    }

    pub fn checked_neg(self) -> Option<Money> {
        self.0.checked_neg().map(Money)
    }

    /// `None` if any partial sum overflows.
    pub fn checked_sum(amounts: impl IntoIterator<Item = Money>) -> Option<Money> {
        amounts
            .into_iter()
            .try_fold(Money::ZERO, |total, amount| total.checked_add(amount))
    }
}

/// Renders as `-12.34` / `0.05`. No currency symbol: the core is
/// currency-agnostic, so presentation layers add one.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        // unsigned_abs so i64::MIN doesn't overflow on negation.
        let abs = self.0.unsigned_abs();
        write!(
            f,
            "{sign}{}.{:02}",
            abs / CENTS_PER_DOLLAR,
            abs % CENTS_PER_DOLLAR
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn display_formats_dollars_and_cents() {
        let cases = [
            (0, "0.00"),
            (5, "0.05"),
            (-5, "-0.05"),
            (100, "1.00"),
            (-1234, "-12.34"),
            (i64::MAX, "92233720368547758.07"),
            (i64::MIN, "-92233720368547758.08"),
        ];
        for (cents, expected) in cases {
            assert_eq!(Money(cents).to_string(), expected, "cents = {cents}");
        }
    }

    #[test]
    fn checked_ops_report_overflow() {
        assert_eq!(Money(i64::MAX).checked_add(Money(1)), None);
        assert_eq!(Money(i64::MIN).checked_sub(Money(1)), None);
        assert_eq!(Money(i64::MIN).checked_neg(), None);
        assert_eq!(Money::checked_sum([Money(i64::MAX), Money(1)]), None);
        assert_eq!(Money::checked_sum([]), Some(Money::ZERO));
    }

    #[test]
    fn serializes_as_bare_integer() {
        let json = serde_json::to_string(&Money(-1234)).unwrap();
        assert_eq!(json, "-1234");
        assert_eq!(serde_json::from_str::<Money>(&json).unwrap(), Money(-1234));
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config())]

        #[test]
        fn checked_ops_match_i64(a in any::<i64>(), b in any::<i64>()) {
            prop_assert_eq!(Money(a).checked_add(Money(b)), a.checked_add(b).map(Money));
            prop_assert_eq!(Money(a).checked_sub(Money(b)), a.checked_sub(b).map(Money));
            prop_assert_eq!(Money(a).checked_neg(), a.checked_neg().map(Money));
        }

        #[test]
        fn display_round_trips_to_cents(cents in any::<i64>()) {
            let digits = Money(cents).to_string().replace('.', "");
            prop_assert_eq!(digits.parse::<i64>(), Ok(cents));
        }
    }
}
