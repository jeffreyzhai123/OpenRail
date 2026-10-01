use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::money::Money;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerSnapshot {
    pub accounts: BTreeMap<String, Money>,
}
