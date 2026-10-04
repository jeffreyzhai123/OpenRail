//! Simulated time. Nothing in sim-core reads a wall clock: time only moves
//! when the simulator advances it to the next event.

use std::fmt;

/// Simulated milliseconds since the run started. One tick is one ms, so
/// scenarios, `Delay` faults and the UI all agree on what a time means.
#[derive(Debug, Default)]
pub struct VirtualClock {
    now: u64,
}

impl VirtualClock {
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Staying at the same tick is fine: several events can share one.
    /// Moving backwards is reported, and `now` stays put, so the caller
    /// decides what an event from the past means.
    pub fn advance_to(&mut self, t: u64) -> Result<(), ClockError> {
        if t < self.now {
            return Err(ClockError::Backwards {
                now: self.now,
                requested: t,
            });
        }
        self.now = t;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    Backwards { now: u64, requested: u64 },
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClockError::Backwards { now, requested } => {
                write!(
                    f,
                    "clock is at {now} ms and can't move back to {requested} ms"
                )
            }
        }
    }
}

impl std::error::Error for ClockError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero() {
        assert_eq!(VirtualClock::default().now(), 0);
    }

    #[test]
    fn advances_forward_and_allows_the_same_tick() {
        let mut clock = VirtualClock::default();
        assert_eq!(clock.advance_to(1_000), Ok(()));
        assert_eq!(clock.advance_to(1_000), Ok(()));
        assert_eq!(clock.now(), 1_000);
    }

    #[test]
    fn moving_backwards_is_an_error_and_keeps_the_time() {
        let mut clock = VirtualClock::default();
        clock.advance_to(1_000).unwrap();
        assert_eq!(
            clock.advance_to(999),
            Err(ClockError::Backwards {
                now: 1_000,
                requested: 999
            })
        );
        assert_eq!(clock.now(), 1_000);
    }
}
