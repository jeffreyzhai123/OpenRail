//! An ACH debit returned days after it settled, and the return redelivered.

use sim_core::event::{EventId, EventKind};
use sim_core::fault::FaultOp;
use sim_core::money::Money;
use sim_core::rails::ach::{AchEntryId, AchEvent, AchReturnCode};

use super::{MS_PER_DAY, MS_PER_HOUR, Scenario, event, opening};

const ENTRY: AchEntryId = AchEntryId(1);
const AMOUNT: Money = Money(20_000);
/// The return's event id, which the story plan redelivers.
const RETURN: u64 = 4;

pub(crate) fn scenario() -> Scenario {
    Scenario {
        id: "late-ach-return",
        name: "ACH return after settlement",
        description: "A $200.00 ACH debit is initiated, batched, and settled the next day. \
            Two days after settling, it comes back as returned (R01, insufficient funds): ACH \
            returns can land days after settlement. The bank then redelivers the return \
            notification, and a handler that doesn't deduplicate reverses the payment twice.",
        initial_ledger: opening(),
        workload: vec![
            event(
                1,
                0,
                EventKind::Ach(AchEvent::Initiated {
                    entry_id: ENTRY,
                    amount: AMOUNT,
                }),
            ),
            event(
                2,
                4 * MS_PER_HOUR,
                EventKind::Ach(AchEvent::Batched { entry_id: ENTRY }),
            ),
            event(
                3,
                MS_PER_DAY,
                EventKind::Ach(AchEvent::Settled { entry_id: ENTRY }),
            ),
            event(
                RETURN,
                3 * MS_PER_DAY,
                EventKind::Ach(AchEvent::Returned {
                    entry_id: ENTRY,
                    code: AchReturnCode::R01,
                    amount: AMOUNT,
                }),
            ),
        ],
        story_plan: vec![FaultOp::Duplicate {
            event_id: EventId(RETURN),
        }],
    }
}
