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
    // Preserve the established millisecond schedule for whole-millisecond
    // policies while retaining precision when either configured bound is finer.
    let quantum_nanos = if policy.base_retry_delay.as_nanos().is_multiple_of(1_000_000)
        && policy.max_retry_delay.as_nanos().is_multiple_of(1_000_000)
    {
        1_000_000
    } else {
        1
    };
    let exponential = (policy.base_retry_delay.as_nanos() / quantum_nanos)
        .checked_shl(exponent)
        .unwrap_or(u128::MAX)
        .min(policy.max_retry_delay.as_nanos() / quantum_nanos);
    let lower = exponential
        .div_ceil(2)
        .max(Duration::from_millis(1).as_nanos() / quantum_nanos);
    let width = exponential.saturating_sub(lower).saturating_add(1);
    let jitter = u128::from(identity_hash(message_key, attempt)) % width;
    let nanoseconds = lower.saturating_add(jitter).saturating_mul(quantum_nanos);
    // Validated policy caps this at 24 hours, well below u64::MAX nanoseconds.
    u64::try_from(nanoseconds)
        .map(Duration::from_nanos)
        .unwrap_or(policy.max_retry_delay)
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
    use edgeagent_messaging::RetryDelay;
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
        assert_eq!(first, Duration::from_millis(1_835));
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

    #[test]
    fn retry_delay_preserves_fractional_milliseconds() -> Result<(), HandlerErrorKind> {
        let policy = HandlerPolicy::new(
            "execution_simulator_v1",
            3,
            Duration::from_micros(1_500),
            Duration::from_micros(1_500),
        )
        .map_err(|error| error.kind())?;
        let delays = (0..32)
            .map(|index| retry_delay(&policy, &format!("delivery-{index}"), 1))
            .collect::<Vec<_>>();

        assert!(delays.iter().all(|delay| {
            *delay >= Duration::from_millis(1) && *delay <= Duration::from_micros(1_500)
        }));
        assert!(delays.iter().all(|delay| RetryDelay::new(*delay).is_ok()));
        assert!(delays.iter().any(|delay| *delay > Duration::from_millis(1)));
        assert!(
            delays
                .iter()
                .any(|delay| !delay.subsec_nanos().is_multiple_of(1_000_000))
        );
        Ok(())
    }
}
