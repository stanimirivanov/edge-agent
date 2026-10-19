use chrono::{DateTime, Datelike};
use chrono_tz::America::New_York;

use crate::{DerivativeError, SessionDate, UtcInstant};

/// Explicit contract deadlines, not inferred exchange trading sessions. UTC
/// milliseconds retain their existing meaning; no input is rounded into them.
/// New York local-date conversion uses the exact bundled IANA revision.
///
/// ```
/// use edgeagent_market_data_domain::{OptionSchedule, UtcInstant};
/// let start = UtcInstant::from_unix_millis(0)?;
/// let end = UtcInstant::from_unix_millis(1000)?;
/// let schedule = OptionSchedule::new(start, end, end, end, end,
///     OptionSchedule::timezone_rules().1)?;
/// assert!(schedule.within_trading_lifetime(start));
/// assert!(!schedule.within_trading_lifetime(end));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionSchedule {
    activation: UtcInstant,
    last_trading: UtcInstant,
    expiration: UtcInstant,
    exercise_cutoff: UtcInstant,
    settlement: UtcInstant,
    expiration_date: SessionDate,
}
impl OptionSchedule {
    /// Check supplied deadlines and the exact timezone-data revision.
    /// Last trading may precede expiration by days; exercise cutoff may differ
    /// from expiration. Cash/delivery settlement follows both final deadlines.
    ///
    /// # Errors
    /// Returns `TimezoneVersion` for a revision mismatch, `Schedule` unless
    /// activation < last trading <= expiration and exercise cutoff, and settlement
    /// >= both final deadlines, or `LocalDate` outside supported local years.
    pub fn new(
        activation: UtcInstant,
        last_trading: UtcInstant,
        expiration: UtcInstant,
        exercise_cutoff: UtcInstant,
        settlement: UtcInstant,
        timezone_revision: &str,
    ) -> Result<Self, DerivativeError> {
        if timezone_revision != chrono_tz::IANA_TZDB_VERSION {
            return Err(DerivativeError::TimezoneVersion);
        }
        if activation >= last_trading
            || last_trading > expiration
            || last_trading > exercise_cutoff
            || settlement < expiration
            || settlement < exercise_cutoff
        {
            return Err(DerivativeError::Schedule);
        }
        for instant in [activation, last_trading, exercise_cutoff, settlement] {
            local_date(instant)?;
        }
        Ok(Self {
            activation,
            last_trading,
            expiration,
            exercise_cutoff,
            settlement,
            expiration_date: local_date(expiration)?,
        })
    }
    /// Supported timezone and bundled IANA revision. Persist both in future wire
    /// specifications/manifests; decoding must validate, not silently replace them.
    #[must_use]
    pub const fn timezone_rules() -> (&'static str, &'static str) {
        ("America/New_York", chrono_tz::IANA_TZDB_VERSION)
    }
    /// Inclusive activation and exclusive last-trading instant. This lifetime
    /// alone does not prove an open calendar session, entitlement or order eligibility.
    #[must_use]
    pub const fn trading_bounds(self) -> (UtcInstant, UtcInstant) {
        (self.activation, self.last_trading)
    }
    /// Contract expiration, distinct from last trading and exercise cutoff.
    #[must_use]
    pub const fn expiration(self) -> UtcInstant {
        self.expiration
    }
    /// Final exercise deadline, not an implementation of American exercise.
    #[must_use]
    pub const fn exercise_cutoff(self) -> UtcInstant {
        self.exercise_cutoff
    }
    /// Contractual settlement instant, not proof that settlement evidence arrived.
    #[must_use]
    pub const fn settlement(self) -> UtcInstant {
        self.settlement
    }
    /// Expiration's checked New York date, never its UTC date or a 24-hour countdown.
    #[must_use]
    pub const fn expiration_date(self) -> SessionDate {
        self.expiration_date
    }
    /// Whether the instant falls inside the contract lifetime, not a market session.
    #[must_use]
    pub fn within_trading_lifetime(self, at: UtcInstant) -> bool {
        self.activation <= at && at < self.last_trading
    }
    /// Whether the instant shares the local expiration date. Does not imply that
    /// last trading has not passed; combine with separately qualified session rules.
    ///
    /// # Errors
    /// Returns `LocalDate` when New York conversion leaves local years 1..=9999.
    pub fn is_zero_dte_at(self, at: UtcInstant) -> Result<bool, DerivativeError> {
        Ok(local_date(at)? == self.expiration_date)
    }
}

fn local_date(at: UtcInstant) -> Result<SessionDate, DerivativeError> {
    let date = DateTime::from_timestamp_millis(at.unix_millis())
        .ok_or(DerivativeError::LocalDate)?
        .with_timezone(&New_York);
    SessionDate::new(date.year(), date.month(), date.day()).map_err(|_| DerivativeError::LocalDate)
}
