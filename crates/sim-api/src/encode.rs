//! Replay links (README §6.2). A link encodes a run's whole input, scenario,
//! seed, handler and the explicit fault plan, so replaying it never
//! regenerates anything. The format is `<version>.<base64url JSON>`: the
//! version sits outside the payload, so a later version can compress the
//! payload and still be told apart (decision V). V1 doesn't compress.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sim_core::fault::FaultPlan;
use sim_core::handlers::HandlerKind;

/// The only version this server reads and writes.
pub const ENCODING_VERSION: &str = "1";

/// The longest plan any request may carry. It keeps replay links short,
/// since V1 doesn't compress: realistic plans at the cap encode to about 6 KB.
pub const MAX_PLAN_FAULTS: usize = 100;

/// Rejects absurd replay strings before decoding. Even a worst-case plan at
/// `MAX_PLAN_FAULTS`, with every number at its maximum, fits.
pub const MAX_REPLAY_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub scenario_id: String,
    pub seed: u32,
    pub handler: HandlerKind,
    pub fault_plan: FaultPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooLong,
    /// Not `<version>.<base64url>`, not base64url, or not a replay.
    Malformed,
    /// A well-formed version this server doesn't read.
    UnsupportedVersion(String),
}

/// Canonical: the same replay always encodes to the same string.
pub fn encode_run(replay: &Replay) -> Result<String, serde_json::Error> {
    let json = serde_json::to_vec(replay)?;
    Ok(format!(
        "{ENCODING_VERSION}.{}",
        URL_SAFE_NO_PAD.encode(json)
    ))
}

pub fn decode_run(encoded: &str) -> Result<Replay, DecodeError> {
    if encoded.len() > MAX_REPLAY_LEN {
        return Err(DecodeError::TooLong);
    }
    let (version, payload) = encoded.split_once('.').ok_or(DecodeError::Malformed)?;
    if version != ENCODING_VERSION {
        let is_version = !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit());
        return Err(if is_version {
            DecodeError::UnsupportedVersion(version.to_string())
        } else {
            DecodeError::Malformed
        });
    }
    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| DecodeError::Malformed)?;
    serde_json::from_slice(&json).map_err(|_| DecodeError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::event::EventId;
    use sim_core::fault::FaultOp;

    fn replay(handler: HandlerKind, fault_plan: FaultPlan) -> Replay {
        Replay {
            scenario_id: "charge-retry".to_string(),
            seed: 42,
            handler,
            fault_plan,
        }
    }

    fn every_op() -> FaultPlan {
        vec![
            FaultOp::Duplicate {
                event_id: EventId(2),
            },
            FaultOp::Reorder {
                event_id: EventId(1),
                window: 3,
            },
            FaultOp::Delay {
                event_id: EventId(3),
                by: 45_000,
            },
            FaultOp::Drop {
                event_id: EventId(4),
            },
            FaultOp::CrashRestart { at: 1_000 },
        ]
    }

    #[test]
    fn decoding_an_encoded_replay_gives_it_back() {
        for handler in [HandlerKind::Naive, HandlerKind::Hardened] {
            for plan in [FaultPlan::new(), every_op()] {
                let original = replay(handler, plan);
                let encoded = encode_run(&original).unwrap();
                assert_eq!(decode_run(&encoded), Ok(original));
            }
        }
    }

    #[test]
    fn encoding_is_canonical_and_url_safe() {
        let encoded = encode_run(&replay(HandlerKind::Naive, every_op())).unwrap();
        assert_eq!(
            encode_run(&replay(HandlerKind::Naive, every_op())).unwrap(),
            encoded
        );
        assert!(encoded.starts_with("1."), "{encoded}");
        assert!(
            encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
            "{encoded}"
        );
    }

    #[test]
    fn a_worst_case_plan_at_the_cap_still_fits() {
        let worst: FaultPlan = (0..MAX_PLAN_FAULTS)
            .map(|_| FaultOp::Reorder {
                event_id: EventId(u64::MAX),
                window: usize::MAX,
            })
            .collect();
        let encoded = encode_run(&replay(HandlerKind::Hardened, worst)).unwrap();
        assert!(encoded.len() <= MAX_REPLAY_LEN, "{}", encoded.len());
    }

    #[test]
    fn an_unknown_version_is_unsupported_not_malformed() {
        let current = encode_run(&replay(HandlerKind::Naive, every_op())).unwrap();
        let future = current.replacen("1.", "2.", 1);
        assert_eq!(
            decode_run(&future),
            Err(DecodeError::UnsupportedVersion("2".to_string()))
        );
    }

    #[test]
    fn malformed_strings_are_rejected() {
        let not_a_replay = format!("1.{}", URL_SAFE_NO_PAD.encode(br#"{"seed":1}"#));
        let unknown_field = format!(
            "1.{}",
            URL_SAFE_NO_PAD.encode(
                br#"{"scenario_id":"x","seed":1,"handler":"naive","fault_plan":[],"extra":true}"#
            )
        );
        for bad in [
            "",
            "no-version-prefix",
            "v1.abc",
            ".abc",
            "1.not*base64",
            "1.e30=",
            &not_a_replay,
            &unknown_field,
        ] {
            assert_eq!(decode_run(bad), Err(DecodeError::Malformed), "{bad}");
        }
    }

    #[test]
    fn an_over_long_string_is_rejected_before_decoding() {
        let long = format!("1.{}", "A".repeat(MAX_REPLAY_LEN));
        assert_eq!(decode_run(&long), Err(DecodeError::TooLong));
    }
}
