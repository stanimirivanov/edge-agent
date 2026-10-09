//! Validated scheduling values shared by outbound storage adapters.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// Prevalidated lifetime of an outbound claim, from 1 millisecond to 15 minutes.
///
/// The value preserves sub-millisecond precision. An adapter must document any
/// coarser persistence precision; PostgreSQL floors to whole milliseconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LeaseDuration(Duration);

impl LeaseDuration {
    /// Minimum supported claim lifetime.
    pub const MIN: Duration = Duration::from_millis(1);
    /// Maximum supported claim lifetime.
    pub const MAX: Duration = Duration::from_secs(15 * 60);

    /// Validate a claim lifetime before requesting storage work.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxTimingError::InvalidLeaseDuration`] outside the inclusive
    /// [`Self::MIN`]–[`Self::MAX`] range, including adjacent nanosecond values.
    pub fn new(duration: Duration) -> Result<Self, OutboxTimingError> {
        if duration < Self::MIN || duration > Self::MAX {
            Err(OutboxTimingError::InvalidLeaseDuration)
        } else {
            Ok(Self(duration))
        }
    }

    /// Return the validated lifetime without rounding.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl TryFrom<Duration> for LeaseDuration {
    type Error = OutboxTimingError;

    fn try_from(duration: Duration) -> Result<Self, Self::Error> {
        Self::new(duration)
    }
}

impl From<LeaseDuration> for Duration {
    fn from(duration: LeaseDuration) -> Self {
        duration.get()
    }
}

/// Prevalidated outbound retry delay, from immediate eligibility to 24 hours.
///
/// Unlike broker [`crate::RetryDelay`], zero is valid: releasing an outbound
/// claim may make it eligible immediately. Sub-millisecond values remain valid
/// and are preserved here; PostgreSQL floors them to whole milliseconds.
///
/// ```
/// use edgeagent_messaging::{OutboxRetryDelay, OutboxTimingError};
/// use std::time::Duration;
///
/// let delayed = OutboxRetryDelay::try_from(Duration::from_secs(2))?;
/// assert!(delayed > OutboxRetryDelay::IMMEDIATE);
/// assert_eq!(Duration::from(delayed), Duration::from_secs(2));
/// # Ok::<(), OutboxTimingError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OutboxRetryDelay(Duration);

impl OutboxRetryDelay {
    /// Minimum supported outbound retry delay.
    pub const MIN: Duration = Duration::ZERO;
    /// Maximum supported outbound retry delay.
    pub const MAX: Duration = Duration::from_secs(24 * 60 * 60);
    /// Release the claim without delaying its next eligibility.
    pub const IMMEDIATE: Self = Self(Duration::ZERO);

    /// Validate an outbound delay before requesting a retry transition.
    ///
    /// # Errors
    ///
    /// Returns [`OutboxTimingError::InvalidRetryDelay`] above [`Self::MAX`].
    pub fn new(duration: Duration) -> Result<Self, OutboxTimingError> {
        if duration > Self::MAX {
            Err(OutboxTimingError::InvalidRetryDelay)
        } else {
            Ok(Self(duration))
        }
    }

    /// Return the validated delay without rounding.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl TryFrom<Duration> for OutboxRetryDelay {
    type Error = OutboxTimingError;

    fn try_from(duration: Duration) -> Result<Self, Self::Error> {
        Self::new(duration)
    }
}

impl From<OutboxRetryDelay> for Duration {
    fn from(duration: OutboxRetryDelay) -> Self {
        duration.get()
    }
}

/// Caller-side timing validation failure, before any outbox storage operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxTimingError {
    /// The requested lease is outside 1 millisecond through 15 minutes.
    InvalidLeaseDuration,
    /// The requested retry delay exceeds 24 hours.
    InvalidRetryDelay,
}

impl Display for OutboxTimingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLeaseDuration => {
                "outbox lease duration must be between 1 millisecond and 15 minutes"
            }
            Self::InvalidRetryDelay => "outbox retry delay must not exceed 24 hours",
        })
    }
}

impl Error for OutboxTimingError {}

#[cfg(test)]
mod tests;
