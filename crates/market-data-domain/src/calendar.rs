use crate::{
    ReferenceError, ReferenceHistory, ReferenceVersion, SessionDate, UtcInstant, Venue,
    reference::{exactly_one, token},
};

/// Ordered UTC core-session bounds; does not describe a provider-day bar bucket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionWindow {
    open: UtcInstant,
    close: UtcInstant,
}
impl SessionWindow {
    /// Construct an explicit window; no clock or timezone inference.
    ///
    /// # Errors
    /// Returns `InvalidInterval` unless open precedes close on the same UTC date.
    /// Supported venues' core sessions fit within that date.
    pub fn new(open: UtcInstant, close: UtcInstant) -> Result<Self, ReferenceError> {
        if open >= close || open.utc_date() != close.utc_date() {
            return Err(ReferenceError::InvalidInterval);
        }
        Ok(Self { open, close })
    }
    /// Inclusive open and exclusive close, both UTC instants.
    #[must_use]
    pub const fn bounds(self) -> (UtcInstant, UtcInstant) {
        (self.open, self.close)
    }
}

/// Explicit reviewed day classification; absent coverage is never `Closed`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionHours {
    /// Explicit weekend, holiday or reviewed closure notice.
    Closed,
    /// Full core session for this date and calendar revision.
    Regular(SessionWindow),
    /// Explicitly shortened core session.
    EarlyClose(SessionWindow),
}

/// One reviewed calendar date. Only supplied dates constitute coverage.
/// UTC windows already encode reviewed DST rules; the ingestion boundary must
/// verify them against the named tzdata and exchange notices. No rule extrapolation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarDay {
    venue: Venue,
    date: SessionDate,
    hours: SessionHours,
    tzdata_revision: String,
}
impl CalendarDay {
    /// Pin an explicit day to `America/New_York` and a named timezone-data revision.
    ///
    /// # Errors
    /// Returns `InvalidToken` for an invalid 1–32-byte revision token or
    /// `InvalidInterval` if an open session's UTC date differs from the local date.
    pub fn new(
        venue: Venue,
        date: SessionDate,
        hours: SessionHours,
        tzdata_revision: &str,
    ) -> Result<Self, ReferenceError> {
        if !token(tzdata_revision, 32) {
            return Err(ReferenceError::InvalidToken);
        }
        if let SessionHours::Regular(window) | SessionHours::EarlyClose(window) = hours
            && window.open.utc_date() != date
        {
            return Err(ReferenceError::InvalidInterval);
        }
        Ok(Self {
            venue,
            date,
            hours,
            tzdata_revision: tzdata_revision.to_owned(),
        })
    }
    /// Venue and exchange-local date covered by this observation.
    #[must_use]
    pub const fn key(&self) -> (Venue, SessionDate) {
        (self.venue, self.date)
    }
    /// Reviewed classification, not provider aggregate scope.
    #[must_use]
    pub const fn hours(&self) -> SessionHours {
        self.hours
    }
    /// Fixed exchange timezone and the supplied timezone-data revision.
    #[must_use]
    pub fn timezone(&self) -> (&'static str, &str) {
        ("America/New_York", &self.tzdata_revision)
    }
}
impl ReferenceHistory<CalendarDay> {
    /// Resolve one explicitly covered day at a knowledge cutoff.
    ///
    /// # Errors
    /// Returns `Missing` for absent/future-known dates or `Ambiguous` for conflicting records.
    pub fn resolve_day(
        &self,
        venue: Venue,
        date: SessionDate,
        cutoff: UtcInstant,
    ) -> Result<&ReferenceVersion<CalendarDay>, ReferenceError> {
        exactly_one(self.at(cutoff).filter(|v| v.value().key() == (venue, date)))
    }
}
