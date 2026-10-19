use crate::{ObservationError, UtcInstant};

/// Exact nanosecond-capable UTC time for new evidence profiles. Existing daily
/// millisecond APIs are unchanged. No clock, rounding or leap-second inference.
///
/// ```
/// use edgeagent_market_data_domain::PreciseInstant;
/// let before_epoch = PreciseInstant::from_unix_parts(-1, 999_999_999)?;
/// assert_eq!(before_epoch.unix_parts(), (-1, 999_999_999));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PreciseInstant {
    millis: UtcInstant,
    submillis: u32,
}
impl PreciseInstant {
    /// Accept Unix seconds and `0..1_000_000_000` nanoseconds within the second.
    ///
    /// # Errors
    /// Returns `Timestamp` for invalid fractions, overflow or UTC years outside 1..9999.
    pub fn from_unix_parts(seconds: i64, nanos: u32) -> Result<Self, ObservationError> {
        if nanos >= 1_000_000_000 {
            return Err(ObservationError::Timestamp);
        }
        let millis = seconds
            .checked_mul(1000)
            .and_then(|s| s.checked_add(i64::from(nanos / 1_000_000)))
            .ok_or(ObservationError::Timestamp)?;
        Ok(Self {
            millis: UtcInstant::from_unix_millis(millis)
                .map_err(|_| ObservationError::Timestamp)?,
            submillis: nanos % 1_000_000,
        })
    }
    /// Losslessly promote an already checked millisecond timestamp.
    #[must_use]
    pub const fn from_millis(millis: UtcInstant) -> Self {
        Self {
            millis,
            submillis: 0,
        }
    }
    /// Return exact Unix seconds and nanoseconds, including before the epoch.
    #[must_use]
    pub fn unix_parts(self) -> (i64, u64) {
        let ms = self.millis.unix_millis();
        let nanos = ms.rem_euclid(1000).unsigned_abs() * 1_000_000 + u64::from(self.submillis);
        (ms.div_euclid(1000), nanos)
    }
    pub(crate) const fn millisecond_floor(self) -> UtcInstant {
        self.millis
    }
    pub(crate) const fn submillis(self) -> u32 {
        self.submillis
    }
}

/// Nonempty half-open high-resolution evidence interval, not an exchange session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceInterval {
    start: PreciseInstant,
    end: PreciseInstant,
}
impl EvidenceInterval {
    /// Preserve exact start/end instants without rounding to daily milliseconds.
    ///
    /// # Errors
    /// Returns `TimeOrder` unless start strictly precedes end.
    pub fn new(start: PreciseInstant, end: PreciseInstant) -> Result<Self, ObservationError> {
        if start >= end {
            return Err(ObservationError::TimeOrder);
        }
        Ok(Self { start, end })
    }
    /// Inclusive start and exclusive end.
    #[must_use]
    pub const fn bounds(self) -> (PreciseInstant, PreciseInstant) {
        (self.start, self.end)
    }
}

/// Causal high-resolution evidence times. Receipt is actual local knowledge;
/// provider timestamps do not prove historical availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceTimes {
    event: PreciseInstant,
    observed: Option<PreciseInstant>,
    published: Option<PreciseInstant>,
    received: PreciseInstant,
}
impl EvidenceTimes {
    /// Require event <= each present provider time <= local receipt.
    ///
    /// # Errors
    /// Returns `TimeOrder` on a contradiction; never repairs clock skew.
    pub fn new(
        event: PreciseInstant,
        observed: Option<PreciseInstant>,
        published: Option<PreciseInstant>,
        received: PreciseInstant,
    ) -> Result<Self, ObservationError> {
        let mut previous = event;
        for at in [observed, published, Some(received)].into_iter().flatten() {
            if at < previous {
                return Err(ObservationError::TimeOrder);
            }
            previous = at;
        }
        Ok(Self {
            event,
            observed,
            published,
            received,
        })
    }
    /// Source event time, distinct from local receipt.
    #[must_use]
    pub const fn event_at(self) -> PreciseInstant {
        self.event
    }
    /// Provider observation/publication times, preserving absence independently.
    #[must_use]
    pub const fn provider_times(self) -> (Option<PreciseInstant>, Option<PreciseInstant>) {
        (self.observed, self.published)
    }
    /// Actual local receipt, never backdated to the source event.
    #[must_use]
    pub const fn received_at(self) -> PreciseInstant {
        self.received
    }
}
