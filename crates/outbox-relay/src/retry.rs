//! Deterministic bounded retry and terminal-failure classification.

use crate::{QuarantineReason, RelayPolicy};
use edgeagent_messaging::PublishErrorKind;
use std::time::Duration;

pub(super) enum RetryDecision {
    Retry {
        delay: Duration,
        failure_code: &'static str,
    },
    Quarantine(QuarantineReason),
}

pub(super) fn retry_decision(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
    failure: PublishErrorKind,
) -> RetryDecision {
    match failure {
        PublishErrorKind::Contract => RetryDecision::Quarantine(QuarantineReason::PublishContract),
        PublishErrorKind::Rejected => {
            RetryDecision::Quarantine(QuarantineReason::TransportRejected)
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown
            if attempt >= policy.max_attempts =>
        {
            RetryDecision::Quarantine(match failure {
                PublishErrorKind::Unavailable => QuarantineReason::AttemptsExhaustedUnavailable,
                _ => QuarantineReason::AttemptsExhaustedConfirmationUnknown,
            })
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown => {
            RetryDecision::Retry {
                delay: retry_delay(policy, message_source, message_id, attempt),
                failure_code: match failure {
                    PublishErrorKind::Unavailable => "transport_unavailable",
                    _ => "confirmation_unknown",
                },
            }
        }
    }
}

pub(super) fn retry_delay(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
) -> Duration {
    let exponent = attempt.saturating_sub(1).min(63);
    let base_milliseconds = policy.base_retry_delay.as_millis();
    let exponential = base_milliseconds
        .checked_shl(exponent)
        .unwrap_or(u128::MAX)
        .min(policy.max_retry_delay.as_millis());
    let lower = exponential.div_ceil(2);
    let width = exponential.saturating_sub(lower).saturating_add(1);
    let jitter = u128::from(identity_hash(message_source, message_id, attempt)) % width;
    let milliseconds = lower.saturating_add(jitter);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX))
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
