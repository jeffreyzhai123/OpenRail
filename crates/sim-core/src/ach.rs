use std::fmt;

use serde::{Deserialize, Serialize};

/// The README's linear ACH lifecycle. Pure: no clock, no ledger. Handlers
/// decide what money moves on each transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AchState {
    Initiated,
    Batched,
    Settled,
    /// Terminal. Returns can land days after settlement, which is exactly the
    /// late-event case the simulator exercises.
    Returned,
}

impl AchState {
    pub const ALL: [AchState; 4] = [
        AchState::Initiated,
        AchState::Batched,
        AchState::Settled,
        AchState::Returned,
    ];

    /// Takes the target state because webhooks report "it is now settled",
    /// not an abstract event. A self-transition (a duplicate webhook) is an
    /// error too: this reports it, and the handler decides whether `from ==
    /// to` is a harmless duplicate.
    pub fn transition(self, to: AchState) -> Result<AchState, AchError> {
        match (self, to) {
            (AchState::Initiated, AchState::Batched)
            | (AchState::Batched, AchState::Settled)
            | (AchState::Settled, AchState::Returned) => Ok(to),
            _ => Err(AchError::IllegalTransition { from: self, to }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchError {
    IllegalTransition { from: AchState, to: AchState },
}

impl fmt::Display for AchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AchError::IllegalTransition { from, to } => {
                write!(f, "illegal ACH transition {from:?} -> {to:?}")
            }
        }
    }
}

impl std::error::Error for AchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use AchState::*;

    const LEGAL: [(AchState, AchState); 3] = [
        (Initiated, Batched),
        (Batched, Settled),
        (Settled, Returned),
    ];

    #[test]
    fn transition_table_is_exactly_the_linear_chain() {
        for from in AchState::ALL {
            for to in AchState::ALL {
                let expected = if LEGAL.contains(&(from, to)) {
                    Ok(to)
                } else {
                    Err(AchError::IllegalTransition { from, to })
                };
                assert_eq!(from.transition(to), expected, "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn duplicate_webhook_is_reported_not_ignored() {
        for state in AchState::ALL {
            assert_eq!(
                state.transition(state),
                Err(AchError::IllegalTransition {
                    from: state,
                    to: state
                })
            );
        }
    }

    #[test]
    fn returned_is_terminal() {
        assert!(
            AchState::ALL
                .iter()
                .all(|to| Returned.transition(*to).is_err())
        );
    }

    #[test]
    fn full_lifecycle_reaches_returned() {
        let state = Initiated
            .transition(Batched)
            .and_then(|s| s.transition(Settled))
            .and_then(|s| s.transition(Returned));
        assert_eq!(state, Ok(Returned));
    }
}
