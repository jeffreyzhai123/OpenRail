//! The playable scenarios (README §3). A scenario is data: an opening ledger,
//! a workload and a story plan. Its workload alone runs clean under both
//! handlers, and its story plan is the small fault plan that shows its bug
//! (specs/scenarios-plan.md, decision P).

mod scenario1_retry;
mod scenario2_refund_order;
mod scenario3_late_return;

use sim_core::event::{EventId, EventKind, SimEvent};
use sim_core::fault::FaultPlan;

const MS_PER_SECOND: u64 = 1_000;
const MS_PER_HOUR: u64 = 60 * 60 * MS_PER_SECOND;
const MS_PER_DAY: u64 = 24 * MS_PER_HOUR;

// The accounts the handlers post to. Local rather than reaching into
// sim-core's handlers; a test proves they match what the handlers post to.
const EXTERNAL_CARD: &str = "external:card";
const EXTERNAL_BANK: &str = "external:bank";
const MERCHANT: &str = "merchant";

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    /// Frozen: replay links store it.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// `(account, cents)`, the shape `Ledger::open` takes.
    pub initial_ledger: Vec<(String, i64)>,
    pub workload: Vec<SimEvent>,
    /// The fault plan that shows this scenario's bug.
    pub story_plan: FaultPlan,
}

/// Every scenario, in the order the UI lists them. Built fresh on each call,
/// so there's no global state.
pub fn scenarios() -> Vec<Scenario> {
    vec![
        scenario1_retry::scenario(),
        scenario2_refund_order::scenario(),
        scenario3_late_return::scenario(),
    ]
}

pub fn find(id: &str) -> Option<Scenario> {
    scenarios().into_iter().find(|scenario| scenario.id == id)
}

/// Zero balances on every account a handler posts to. External accounts are
/// counterparties, so they can go negative.
fn opening() -> Vec<(String, i64)> {
    [EXTERNAL_CARD, EXTERNAL_BANK, MERCHANT]
        .iter()
        .map(|account| (account.to_string(), 0))
        .collect()
}

/// `seq` stays 0: the event queue assigns it.
fn event(id: u64, time: u64, kind: EventKind) -> SimEvent {
    SimEvent {
        id: EventId(id),
        time,
        seq: 0,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use sim_core::fault::apply_fault_plan;
    use sim_core::handlers::HandlerKind;
    use sim_core::ledger::Ledger;
    use sim_core::rails::ach::{AchEvent, AchState};

    use super::*;

    #[test]
    fn ids_are_frozen_and_listed_in_order() {
        let ids: Vec<&str> = scenarios().iter().map(|scenario| scenario.id).collect();
        assert_eq!(
            ids,
            ["charge-retry", "refund-before-capture", "late-ach-return"]
        );
    }

    #[test]
    fn find_returns_each_scenario_by_id() {
        for scenario in scenarios() {
            assert_eq!(find(scenario.id), Some(scenario));
        }
        assert_eq!(find("no-such-scenario"), None);
    }

    #[test]
    fn openings_are_accepted_by_the_ledger() {
        for scenario in scenarios() {
            let opened = Ledger::open(&scenario.initial_ledger);
            assert!(opened.is_ok(), "{}: {opened:?}", scenario.id);
        }
    }

    #[test]
    fn workload_and_story_plan_both_apply() {
        // Proves the workload's ids are unique, every story target exists,
        // and no fault overflows the clock.
        for scenario in scenarios() {
            let bare = apply_fault_plan(&scenario.workload, &[]);
            let story = apply_fault_plan(&scenario.workload, &scenario.story_plan);
            assert!(bare.is_ok(), "{}: {bare:?}", scenario.id);
            assert!(story.is_ok(), "{}: {story:?}", scenario.id);
        }
    }

    #[test]
    fn every_scenario_has_a_story() {
        for scenario in scenarios() {
            assert!(!scenario.story_plan.is_empty(), "{}", scenario.id);
        }
    }

    #[test]
    fn opening_declares_every_account_a_handler_posts_to() {
        // The naive handler posts on every money event, so its entries cover
        // every account a run could touch.
        for scenario in scenarios() {
            let ledger = Ledger::open(&scenario.initial_ledger).unwrap();
            let declared: BTreeSet<&str> = scenario
                .initial_ledger
                .iter()
                .map(|(account, _)| account.as_str())
                .collect();
            let mut naive = HandlerKind::Naive.build();
            for event in &scenario.workload {
                for entry in naive.handle(event, &ledger) {
                    for posting in &entry.postings {
                        assert!(
                            declared.contains(posting.account.as_str()),
                            "{}: {} isn't in the opening",
                            scenario.id,
                            posting.account
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn ach_events_follow_the_legal_lifecycle() {
        for scenario in scenarios() {
            let mut events: Vec<&SimEvent> = scenario.workload.iter().collect();
            // Stable: same-time events keep workload order, as the queue does.
            events.sort_by_key(|event| event.time);

            let mut states: BTreeMap<u64, AchState> = BTreeMap::new();
            for event in events {
                let EventKind::Ach(ach) = &event.kind else {
                    continue;
                };
                let (entry_id, to) = match ach {
                    AchEvent::Initiated { entry_id, .. } => {
                        let earlier = states.insert(entry_id.0, AchState::Initiated);
                        assert_eq!(
                            earlier, None,
                            "{}: entry {} initiated twice",
                            scenario.id, entry_id.0
                        );
                        continue;
                    }
                    AchEvent::Batched { entry_id } => (entry_id, AchState::Batched),
                    AchEvent::Settled { entry_id } => (entry_id, AchState::Settled),
                    AchEvent::Returned { entry_id, .. } => (entry_id, AchState::Returned),
                };
                let Some(from) = states.get(&entry_id.0).copied() else {
                    panic!(
                        "{}: entry {} has events before Initiated",
                        scenario.id, entry_id.0
                    );
                };
                let next = from.transition(to);
                assert!(next.is_ok(), "{}: {next:?}", scenario.id);
                states.insert(entry_id.0, to);
            }
        }
    }
}
