use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

use crate::rails::ach::AchEvent;
use crate::rails::card::CardEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

// Thin dispatcher over per-rail event vocabularies (crate::rails::{ach,card}).
// Each rail owns its own fields and state machine; adding a rail (e.g. RTP)
// is an additive variant here, not a merge into an unrelated field list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Card(CardEvent),
    Ach(AchEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimEvent {
    pub id: EventId,
    pub time: u64,
    pub seq: u64,
    pub kind: EventKind,
}

impl Ord for SimEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.time, self.seq).cmp(&(other.time, other.seq))
    }
}

impl PartialOrd for SimEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
