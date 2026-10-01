use serde::{Deserialize, Serialize};

use crate::event::EventId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FaultOp {
    Duplicate { event_id: EventId },
    Reorder { window: usize },
    Delay { event_id: EventId, by: u64 },
    Drop { event_id: EventId },
    CrashRestart { at: u64 },
}

pub type FaultPlan = Vec<FaultOp>;
