use serde::{Deserialize, Serialize};

use crate::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChargeId(pub u64);

/// Card rail event vocabulary. No `CardState` yet — V1 scope is ACH only
/// (README §3); these variants exist so `EventKind::Card` compiles and
/// scenarios can be authored, but nothing enforces transition legality the
/// way `AchState::transition` does. Add that once the card rail is in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardEvent {
    Authorized { charge_id: ChargeId, amount: Money },
    Captured { charge_id: ChargeId, amount: Money },
    Refunded { charge_id: ChargeId, amount: Money },
}
