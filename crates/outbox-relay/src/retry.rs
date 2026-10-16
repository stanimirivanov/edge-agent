//! Deterministic bounded retry and terminal-failure classification.

use crate::{QuarantineReason, RelayError, RelayPolicy};
use edgeagent_messaging::{FailureCode, OutboxRetryDelay, PublishErrorKind};
use std::time::Duration;

const TRANSPORT_UNAVAILABLE: FailureCode = FailureCode::from_static("transport_unavailable");
const CONFIRMATION_UNKNOWN: FailureCode = FailureCode::from_static("confirmation_unknown");

pub(super) enum RetryDecision {
    Retry {
        delay: OutboxRetryDelay,
        failure_code: FailureCode,
    },
    Quarantine(QuarantineReason),
}

pub(super) fn retry_decision(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
    failure: PublishErrorKind,
) -> Result<RetryDecision, RelayError> {
    Ok(match failure {
        PublishErrorKind::Contract => RetryDecision::Quarantine(QuarantineReason::PublishContract),
        PublishErrorKind::Rejected => {
            RetryDecision::Quarantine(QuarantineReason::TransportRejected)
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown
            if attempt >= policy.max_attempts.get() =>
        {
            RetryDecision::Quarantine(match failure {
                PublishErrorKind::Unavailable => QuarantineReason::AttemptsExhaustedUnavailable,
                _ => QuarantineReason::AttemptsExhaustedConfirmationUnknown,
            })
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown => {
            RetryDecision::Retry {
                delay: retry_delay(policy, message_source, message_id, attempt)?,
                failure_code: match failure {
                    PublishErrorKind::Unavailable => TRANSPORT_UNAVAILABLE,
                    _ => CONFIRMATION_UNKNOWN,
                },
            }
        }
    })
}

pub(super) fn retry_delay(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
) -> Result<OutboxRetryDelay, RelayError> {
    let exponent = attempt.saturating_sub(1).min(63);
    let base_milliseconds = policy.backoff.base_delay().as_millis();
    // Validated base <= 86_400_000ms and exponent <= 63 keep this product
    // below 2^90, inside u128. Cap before jitter, without overflow sentinels.
    let exponential =
        (base_milliseconds * (1_u128 << exponent)).min(policy.backoff.max_delay().as_millis());
    let lower = exponential.div_ceil(2);
    // Base >= 1ms implies 1 <= lower <= exponential, hence width >= 1.
    let width = exponential - lower + 1;
    let jitter = u128::from(identity_hash(message_source, message_id, attempt)) % width;
    let milliseconds = u64::try_from(lower + jitter).map_err(RelayError::calculation_invariant)?;
    OutboxRetryDelay::new(Duration::from_millis(milliseconds))
        .map_err(RelayError::calculation_invariant)
}

fn identity_hash(message_source: &str, message_id: &str, attempt: u32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in message_source
        .bytes()
        .chain([0])
        .chain(message_id.bytes())
        .chain(attempt.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
