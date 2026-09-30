//! Validated, bounded relay-worker configuration.

use crate::RelayError;
use std::time::Duration;

const MAX_ATTEMPTS: u32 = 100;
const MAX_LEASE_DURATION: Duration = Duration::from_secs(15 * 60);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounded relay configuration shared by all iterations of one worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPolicy {
    pub(super) lease_owner: String,
    pub(super) lease_duration: Duration,
    pub(super) max_attempts: u32,
    pub(super) base_retry_delay: Duration,
    pub(super) max_retry_delay: Duration,
}

impl RelayPolicy {
    /// Construct a validated policy.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPolicy` when the attempt budget or delay bounds cannot
    /// produce bounded exponential backoff or a safe lease.
    pub fn new(
        lease_owner: impl Into<String>,
        lease_duration: Duration,
        max_attempts: u32,
        base_retry_delay: Duration,
        max_retry_delay: Duration,
    ) -> Result<Self, RelayError> {
        let lease_owner = lease_owner.into();
        if lease_owner.is_empty()
            || lease_owner.len() > 128
            || !lease_owner.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(RelayError::invalid_policy(
                "lease_owner must be a lowercase ASCII token of 1 to 128 bytes",
            ));
        }
        if lease_duration < Duration::from_millis(1) || lease_duration > MAX_LEASE_DURATION {
            return Err(RelayError::invalid_policy(
                "lease_duration must be between 1 millisecond and 15 minutes",
            ));
        }
        if max_attempts == 0 || max_attempts > MAX_ATTEMPTS {
            return Err(RelayError::invalid_policy(
                "max_attempts must be between 1 and 100",
            ));
        }
        if base_retry_delay < Duration::from_millis(1) {
            return Err(RelayError::invalid_policy(
                "base_retry_delay must be at least 1 millisecond",
            ));
        }
        if max_retry_delay < base_retry_delay || max_retry_delay > MAX_RETRY_DELAY {
            return Err(RelayError::invalid_policy(
                "max_retry_delay must be at least the base delay and at most 24 hours",
            ));
        }
        Ok(Self {
            lease_owner,
            lease_duration,
            max_attempts,
            base_retry_delay,
            max_retry_delay,
        })
    }
}
