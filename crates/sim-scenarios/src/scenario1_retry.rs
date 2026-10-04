//! A charge whose capture webhook the provider redelivers.

use sim_core::event::{EventId, EventKind};
use sim_core::fault::FaultOp;
use sim_core::money::Money;
use sim_core::rails::card::{CardEvent, ChargeId};

use super::{MS_PER_HOUR, MS_PER_SECOND, Scenario, event, opening};

const CHARGE: ChargeId = ChargeId(1);
const AMOUNT: Money = Money(5_000);
const PARTIAL_REFUND: Money = Money(1_000);
/// The capture's event id, which the story plan redelivers.
const CAPTURE: u64 = 2;

pub(crate) fn scenario() -> Scenario {
    Scenario {
        id: "charge-retry",
        name: "Charge retried after timeout",
        description: "A $50.00 card payment is authorized, captured, and partly refunded an \
            hour later. The provider times out waiting for us to acknowledge the capture \
            webhook, so it redelivers it 30 seconds later with the same event id. A handler \
            that doesn't deduplicate captures the charge twice. This is a provider-side \
            redelivery, not a client retrying with an idempotency key: those are modelled in V4.",
        initial_ledger: opening(),
        workload: vec![
            event(
                1,
                0,
                EventKind::Card(CardEvent::Authorized {
                    charge_id: CHARGE,
                    amount: AMOUNT,
                }),
            ),
            event(
                CAPTURE,
                2 * MS_PER_SECOND,
                EventKind::Card(CardEvent::Captured {
                    charge_id: CHARGE,
                    amount: AMOUNT,
                }),
            ),
            event(
                3,
                MS_PER_HOUR,
                EventKind::Card(CardEvent::Refunded {
                    charge_id: CHARGE,
                    amount: PARTIAL_REFUND,
                }),
            ),
        ],
        story_plan: vec![FaultOp::Duplicate {
            event_id: EventId(CAPTURE),
        }],
    }
}
