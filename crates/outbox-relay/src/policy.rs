//! Startup validation, separate from runtime storage and retry failures.

use edgeagent_messaging::{LeaseDuration, OutboxRetryDelay};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// Startup configuration failure, never returned by a relay iteration.
/// Formatting contains only fixed requirements, not rejected input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayPolicyError {
    /// Worker token is not 1–128 lowercase ASCII letters, digits, `-` or `_`.
    InvalidLeaseOwner,
    /// Inclusive acquired-claim budget is outside 1–100.
    InvalidMaxAttempts,
    /// Backoff does not satisfy `1ms <= base <= maximum <= 24h`.
    InvalidRetryBackoff,
}

impl Display for RelayPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLeaseOwner => {
                "lease_owner must be a lowercase ASCII token of 1 to 128 bytes"
            }
            Self::InvalidMaxAttempts => "max_attempts must be between 1 and 100",
            Self::InvalidRetryBackoff => {
                "retry backoff requires 1 millisecond <= base <= maximum <= 24 hours"
            }
        })
    }
}
impl Error for RelayPolicyError {}

/// Inclusive budget of acquired claims, not physical network requests.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MaxAttempts(u32);

impl MaxAttempts {
    /// Minimum admitted acquired-claim budget.
    pub const MIN: u32 = 1;
    /// Maximum admitted acquired-claim budget.
    pub const MAX: u32 = 100;
    /// Validate a budget before constructing worker policy.
    ///
    /// # Errors
    /// Returns [`RelayPolicyError::InvalidMaxAttempts`] outside inclusive bounds.
    pub fn new(attempts: u32) -> Result<Self, RelayPolicyError> {
        if (Self::MIN..=Self::MAX).contains(&attempts) {
            Ok(Self(attempts))
        } else {
            Err(RelayPolicyError::InvalidMaxAttempts)
        }
    }
    /// Return the validated inclusive budget.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}
impl TryFrom<u32> for MaxAttempts {
    type Error = RelayPolicyError;
    fn try_from(attempts: u32) -> Result<Self, Self::Error> {
        Self::new(attempts)
    }
}
impl From<MaxAttempts> for u32 {
    fn from(attempts: MaxAttempts) -> Self {
        attempts.get()
    }
}

/// Nonzero, ordered, bounded retry configuration.
///
/// Preserve exact configuration precision; scheduling deliberately floors both
/// values to milliseconds before capped exponential equal jitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryBackoff {
    base: Duration,
    maximum: Duration,
}

impl RetryBackoff {
    /// Minimum supported base delay.
    pub const MIN: Duration = Duration::from_millis(1);
    /// Maximum supported delay, shared with the portable outbound port.
    pub const MAX: Duration = OutboxRetryDelay::MAX;
    /// Validate `1ms <= base <= maximum <= 24h` before relay I/O.
    ///
    /// # Errors
    /// Returns [`RelayPolicyError::InvalidRetryBackoff`] for an invalid interval.
    pub fn new(base: Duration, maximum: Duration) -> Result<Self, RelayPolicyError> {
        if base < Self::MIN || maximum < base || maximum > Self::MAX {
            Err(RelayPolicyError::InvalidRetryBackoff)
        } else {
            Ok(Self { base, maximum })
        }
    }
    /// Return the exact, unrounded configured base.
    #[must_use]
    pub const fn base_delay(self) -> Duration {
        self.base
    }
    /// Return the exact, unrounded configured cap.
    #[must_use]
    pub const fn max_delay(self) -> Duration {
        self.maximum
    }
}

/// Validated startup policy shared by all iterations of one worker.
///
/// ```
/// use edgeagent_messaging::LeaseDuration;
/// use edgeagent_outbox_relay::{MaxAttempts, RelayPolicy, RetryBackoff};
/// use std::time::Duration;
/// let policy = RelayPolicy::new(
///     "relay_01", LeaseDuration::new(Duration::from_secs(30))?,
///     MaxAttempts::new(3)?,
///     RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Raw lease durations cannot cross this constructor:
///
/// ```compile_fail,E0308
/// use edgeagent_outbox_relay::{MaxAttempts, RelayPolicy, RetryBackoff};
/// use std::time::Duration;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let _ = RelayPolicy::new("relay_01", Duration::from_secs(30), MaxAttempts::new(3)?,
///     RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?);
/// # Ok(()) }
/// ```
///
/// Raw attempt counts cannot bypass budget validation:
///
/// ```compile_fail,E0308
/// use edgeagent_messaging::LeaseDuration;
/// use edgeagent_outbox_relay::{RelayPolicy, RetryBackoff};
/// use std::time::Duration;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let _ = RelayPolicy::new("relay_01", LeaseDuration::new(Duration::from_secs(30))?, 3,
///     RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?);
/// # Ok(()) }
/// ```
///
/// A pair of durations cannot bypass ordered backoff validation:
///
/// ```compile_fail,E0308
/// use edgeagent_messaging::LeaseDuration;
/// use edgeagent_outbox_relay::{MaxAttempts, RelayPolicy};
/// use std::time::Duration;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let _ = RelayPolicy::new("relay_01", LeaseDuration::new(Duration::from_secs(30))?, MaxAttempts::new(3)?,
///     (Duration::from_secs(1), Duration::from_secs(60)));
/// # Ok(()) }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPolicy {
    pub(super) lease_owner: String,
    pub(super) lease_duration: LeaseDuration,
    pub(super) max_attempts: MaxAttempts,
    pub(super) backoff: RetryBackoff,
}

impl RelayPolicy {
    /// Combine validated timing/budget values with a validated worker token.
    ///
    /// Cancellation consumes committed claims. The final permitted claim may
    /// publish; a later claim quarantines before publication, including after
    /// policy reduction or lost broker results. Lease duration must exceed the
    /// composition root's publication timeout; this constructor cannot infer it.
    ///
    /// # Errors
    /// Returns [`RelayPolicyError::InvalidLeaseOwner`] for an invalid worker token.
    pub fn new(
        lease_owner: impl Into<String>,
        lease_duration: LeaseDuration,
        max_attempts: MaxAttempts,
        backoff: RetryBackoff,
    ) -> Result<Self, RelayPolicyError> {
        let lease_owner = lease_owner.into();
        if lease_owner.is_empty()
            || lease_owner.len() > 128
            || !lease_owner.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(RelayPolicyError::InvalidLeaseOwner);
        }
        Ok(Self {
            lease_owner,
            lease_duration,
            max_attempts,
            backoff,
        })
    }
}
