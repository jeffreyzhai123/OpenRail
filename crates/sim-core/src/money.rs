use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Money(pub i64);

impl Money {
    pub fn checked_add(self, rhs: Money) -> Option<Money> {
        self.0.checked_add(rhs.0).map(Money)
    }

    pub fn checked_sub(self, rhs: Money) -> Option<Money> {
        self.0.checked_sub(rhs.0).map(Money)
    }
}
