use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    Charge,
    Refund,
    AchReturn,
    Webhook,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimEvent {
    pub id: EventId,
    pub time: u64,
    pub seq: u64,
    pub kind: EventKind,
    pub payload: serde_json::Value,
}

impl Eq for SimEvent {}

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
