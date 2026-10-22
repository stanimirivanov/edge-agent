use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::ValueError;

/// UTC instant at millisecond precision, within Gregorian years 1 through 9999.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UtcInstant(DateTime<Utc>);

impl UtcInstant {
    /// Validate an exact Unix millisecond value; never reads the system clock.
    ///
    /// # Errors
    /// Returns `ValueError::Instant` outside the supported UTC year range.
    pub fn from_unix_millis(value: i64) -> Result<Self, ValueError> {
        DateTime::from_timestamp_millis(value)
            .filter(|instant| (1..=9999).contains(&instant.year()))
            .map(Self)
            .ok_or(ValueError::Instant)
    }

    /// Return exact signed milliseconds since the Unix epoch.
    #[must_use]
    pub fn unix_millis(self) -> i64 {
        self.0.timestamp_millis()
    }

    pub(crate) fn utc_date(self) -> SessionDate {
        SessionDate(self.0.date_naive())
    }
}

/// Exchange-local Gregorian date, not proof of an open market or a UTC instant.
/// Venue, timezone revision and calendar membership belong to reference validation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionDate(NaiveDate);

impl SessionDate {
    pub(crate) fn days_until(self, end: Self) -> u64 {
        (end.0 - self.0).num_days().unsigned_abs()
    }
    /// Validate a Gregorian date within years 1 through 9999.
    ///
    /// # Errors
    /// Returns `ValueError::Date` for an invalid date or unsupported year.
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self, ValueError> {
        NaiveDate::from_ymd_opt(year, month, day)
            .filter(|date| (1..=9999).contains(&date.year()))
            .map(Self)
            .ok_or(ValueError::Date)
    }

    /// Return local year, month and day; performs no timezone conversion.
    #[must_use]
    pub fn ymd(self) -> (i32, u32, u32) {
        (self.0.year(), self.0.month(), self.0.day())
    }
}

/// Causal observation times. Provider times remain absent when not supplied.
/// Receipt time is local knowledge, not proof of earlier historical availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationTimes {
    event_at: UtcInstant,
    provider_observed_at: Option<UtcInstant>,
    provider_published_at: Option<UtcInstant>,
    received_at: UtcInstant,
}

impl ObservationTimes {
    /// Validate event <= provider observation <= provider publication <= receipt
    /// for the timestamps present. Contradictions fail; clock skew is not repaired.
    ///
    /// # Errors
    /// Returns `ValueError::TimeOrder` if any present timestamp precedes its predecessor.
    pub fn new(
        event_at: UtcInstant,
        provider_observed_at: Option<UtcInstant>,
        provider_published_at: Option<UtcInstant>,
        received_at: UtcInstant,
    ) -> Result<Self, ValueError> {
        let mut previous = event_at;
        for next in [
            provider_observed_at,
            provider_published_at,
            Some(received_at),
        ]
        .into_iter()
        .flatten()
        {
            if next < previous {
                return Err(ValueError::TimeOrder);
            }
            previous = next;
        }
        Ok(Self {
            event_at,
            provider_observed_at,
            provider_published_at,
            received_at,
        })
    }

    /// Source event instant, separate from receipt and provider timestamps.
    #[must_use]
    pub const fn event_at(self) -> UtcInstant {
        self.event_at
    }
    /// Provider observation time, if explicitly known.
    #[must_use]
    pub const fn provider_observed_at(self) -> Option<UtcInstant> {
        self.provider_observed_at
    }
    /// Provider-declared publication time; not automatically trusted historical proof.
    #[must_use]
    pub const fn provider_published_at(self) -> Option<UtcInstant> {
        self.provider_published_at
    }
    /// Local receipt instant; never inferred from the provider's bar timestamp.
    #[must_use]
    pub const fn received_at(self) -> UtcInstant {
        self.received_at
    }
}
