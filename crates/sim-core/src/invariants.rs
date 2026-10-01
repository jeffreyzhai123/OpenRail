use serde::{Deserialize, Serialize};

// The shrinker's stopping condition is "the *same named* invariant still
// fails" (README §6.1), so a name/identity is required, not just pass/fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantResult {
    pub name: &'static str,
    pub passed: bool,
    pub message: Option<String>,
}
