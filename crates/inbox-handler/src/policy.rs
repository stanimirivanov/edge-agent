use std::time::Duration;

use crate::HandlerError;

const MAX_ATTEMPTS: u32 = 100;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounded retry policy shared by every delivery handled by one logical consumer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandlerPolicy {
    pub(super) consumer_name: String,
    pub(super) max_delivery_attempts: u32,
    pub(super) base_retry_delay: Duration,
    pub(super) max_retry_delay: Duration,
}

impl HandlerPolicy {
    /// Construct a validated consumer policy.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPolicy` for an unstable consumer name or unsafe retry bounds.
    pub fn new(
        consumer_name: impl Into<String>,
        max_delivery_attempts: u32,
        base_retry_delay: Duration,
        max_retry_delay: Duration,
    ) -> Result<Self, HandlerError> {
        let consumer_name = consumer_name.into();
        if consumer_name.is_empty()
            || consumer_name.len() > 128
            || !consumer_name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(HandlerError::invalid_policy(
                "consumer_name must be a lowercase ASCII token of 1 to 128 bytes",
            ));
        }
        if max_delivery_attempts == 0 || max_delivery_attempts > MAX_ATTEMPTS {
            return Err(HandlerError::invalid_policy(
                "max_delivery_attempts must be between 1 and 100",
            ));
        }
        if base_retry_delay < Duration::from_millis(1) {
            return Err(HandlerError::invalid_policy(
                "base_retry_delay must be at least 1 millisecond",
            ));
        }
        if max_retry_delay < base_retry_delay || max_retry_delay > MAX_RETRY_DELAY {
            return Err(HandlerError::invalid_policy(
                "max_retry_delay must be at least the base delay and at most 24 hours",
            ));
        }
        Ok(Self {
            consumer_name,
            max_delivery_attempts,
            base_retry_delay,
            max_retry_delay,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::HandlerPolicy;
    use crate::HandlerErrorKind;
    use std::time::Duration;

    #[test]
    fn policy_and_failure_codes_are_bounded() {
        assert_eq!(
            HandlerPolicy::new(
                "Execution Simulator",
                3,
                Duration::from_secs(1),
                Duration::from_secs(30),
            )
            .err()
            .map(|error| error.kind()),
            Some(HandlerErrorKind::InvalidPolicy)
        );
        assert_eq!(
            HandlerPolicy::new(
                "execution_simulator_v1",
                0,
                Duration::from_secs(1),
                Duration::from_secs(30),
            )
            .err()
            .map(|error| error.kind()),
            Some(HandlerErrorKind::InvalidPolicy)
        );
    }
}
