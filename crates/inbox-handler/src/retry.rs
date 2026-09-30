use std::time::Duration;

use edgeagent_messaging::HandlerFailureKind;

use crate::HandlerPolicy;

pub(super) fn should_retry_handler(
    policy: &HandlerPolicy,
    attempt: u32,
    kind: HandlerFailureKind,
) -> bool {
    kind == HandlerFailureKind::Transient && attempt < policy.max_delivery_attempts
}

pub(super) fn retry_delay(policy: &HandlerPolicy, message_key: &str, attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(63);
    let base_milliseconds = policy.base_retry_delay.as_millis();
    let exponential = base_milliseconds
        .checked_shl(exponent)
        .unwrap_or(u128::MAX)
        .min(policy.max_retry_delay.as_millis());
    let lower = exponential.div_ceil(2);
    let width = exponential.saturating_sub(lower).saturating_add(1);
    let jitter = u128::from(identity_hash(message_key, attempt)) % width;
    let milliseconds = lower.saturating_add(jitter);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX))
}

fn identity_hash(message_key: &str, attempt: u32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in message_key.bytes().chain([0]).chain(attempt.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{retry_delay, should_retry_handler};
    use crate::{HandlerErrorKind, HandlerFailureKind, HandlerPolicy};
    use std::time::Duration;

    #[test]
    fn retry_delay_is_deterministic_bounded_and_identity_jittered() -> Result<(), HandlerErrorKind>
    {
        let policy = HandlerPolicy::new(
            "execution_simulator_v1",
            4,
            Duration::from_secs(2),
            Duration::from_secs(10),
        )
        .map_err(|error| error.kind())?;
        let first = retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 1);
        assert_eq!(first, retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 1));
        assert!(first >= Duration::from_secs(1));
        assert!(first <= Duration::from_secs(2));
        assert_ne!(first, retry_delay(&policy, "18:EDGEAGENT_COMMANDS:8", 1));
        assert!(retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 100) <= Duration::from_secs(10));
        assert!(should_retry_handler(
            &policy,
            3,
            HandlerFailureKind::Transient
        ));
        assert!(!should_retry_handler(
            &policy,
            4,
            HandlerFailureKind::Transient
        ));
        assert!(!should_retry_handler(
            &policy,
            1,
            HandlerFailureKind::Permanent
        ));
        Ok(())
    }
}
