//! A refund whose webhook arrives before its capture's.

use sim_core::event::{EventId, EventKind};
use sim_core::fault::FaultOp;
use sim_core::money::Money;
use sim_core::rails::card::{CardEvent, ChargeId};

use super::{MS_PER_SECOND, Scenario, event, opening};

const CHARGE: ChargeId = ChargeId(2);
const AMOUNT: Money = Money(8_000);
/// The capture's event id. The story plan reverses it with the next delivery.
const CAPTURE: u64 = 2;
/// The capture and the refund right after it.
const SWAP: usize = 2;

pub(crate) fn scenario() -> Scenario {
    Scenario {
        id: "refund-before-capture",
        name: "Refund arrives before capture",
        description: "An $80.00 card payment is captured, then fully refunded half a second \
            later. The provider delivers the two webhooks out of order, so the refund arrives \
            first. A handler that posts in arrival order refunds money it hasn't captured yet. \
            The hardened handler rejects the early refund and posts nothing for it. No V1 \
            invariant notices that the refund is then lost: reconciliation (V4) does.",
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
                MS_PER_SECOND,
                EventKind::Card(CardEvent::Captured {
                    charge_id: CHARGE,
                    amount: AMOUNT,
                }),
            ),
            event(
                3,
                MS_PER_SECOND + MS_PER_SECOND / 2,
                EventKind::Card(CardEvent::Refunded {
                    charge_id: CHARGE,
                    amount: AMOUNT,
                }),
            ),
        ],
        story_plan: vec![FaultOp::Reorder {
            event_id: EventId(CAPTURE),
            window: SWAP,
        }],
    }
}
