use std::fmt;

use crate::{
    InstrumentId, ListingId, ReferenceError, ReferenceHistory, ReferenceVersion, SessionDate,
    UtcInstant,
    reference::{exactly_one, token},
};

/// Supported listing venues; this is not an admission or liquidity decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Venue {
    /// New York Stock Exchange.
    Xnys,
    /// Nasdaq Stock Market.
    Xnas,
    /// NYSE Arca.
    Arcx,
}

/// Bounded, case-sensitive provider alias; never stable instrument identity.
#[derive(Clone, Eq, PartialEq)]
pub struct Symbol(String);
impl Symbol {
    /// Accept 1–32 ASCII alphanumeric, dot, dash or underscore bytes, without
    /// normalization. Provider alias conversion remains an adapter concern.
    ///
    /// # Errors
    /// Returns `InvalidToken` for invalid or oversized input before allocating.
    pub fn new(value: &str) -> Result<Self, ReferenceError> {
        if !token(value, 32) {
            return Err(ReferenceError::InvalidToken);
        }
        Ok(Self(value.to_owned()))
    }
    /// Explicit alias access; diagnostics redact it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Symbol(..)")
    }
}

/// Half-open effective date interval `[start, end)`; unrelated to knowledge time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateRange {
    start: SessionDate,
    end: SessionDate,
}
impl DateRange {
    /// Exact Gregorian calendar-day count; never infers exchange sessions.
    #[must_use]
    pub fn calendar_days(self) -> u64 {
        self.start.days_until(self.end)
    }
    /// Construct a nonempty, bounded effective interval.
    ///
    /// # Errors
    /// Returns `InvalidInterval` unless start precedes end.
    pub fn new(start: SessionDate, end: SessionDate) -> Result<Self, ReferenceError> {
        if start >= end {
            return Err(ReferenceError::InvalidInterval);
        }
        Ok(Self { start, end })
    }
    /// Inclusive effective start and exclusive effective end.
    #[must_use]
    pub const fn bounds(self) -> (SessionDate, SessionDate) {
        (self.start, self.end)
    }
    /// Whether the effective date lies in this interval.
    #[must_use]
    pub fn contains(self, date: SessionDate) -> bool {
        self.start <= date && date < self.end
    }
}

/// One dated alias-to-listing mapping; neither universe admission nor continuity
/// across a reorganization can be inferred from equal ticker text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentMapping {
    instrument: InstrumentId,
    listing: ListingId,
    symbol: Symbol,
    venue: Venue,
    effective: DateRange,
}
impl InstrumentMapping {
    /// Bind checked identities and alias to an explicit venue/effective interval.
    #[must_use]
    pub const fn new(
        instrument: InstrumentId,
        listing: ListingId,
        symbol: Symbol,
        venue: Venue,
        effective: DateRange,
    ) -> Self {
        Self {
            instrument,
            listing,
            symbol,
            venue,
            effective,
        }
    }
    /// Stable instrument and listing identities, respectively.
    #[must_use]
    pub const fn identities(&self) -> (InstrumentId, ListingId) {
        (self.instrument, self.listing)
    }
    /// Alias, venue and effective interval preserved by this observation.
    #[must_use]
    pub const fn binding(&self) -> (&Symbol, Venue, DateRange) {
        (&self.symbol, self.venue, self.effective)
    }
}
impl ReferenceHistory<InstrumentMapping> {
    /// Resolve a unique known mapping without inferring missing historical identity.
    ///
    /// # Errors
    /// Returns `Missing` or `Ambiguous`; never chooses by input order or source rank.
    pub fn resolve_mapping(
        &self,
        symbol: &Symbol,
        venue: Venue,
        date: SessionDate,
        cutoff: UtcInstant,
    ) -> Result<&ReferenceVersion<InstrumentMapping>, ReferenceError> {
        exactly_one(self.at(cutoff).filter(|v| {
            let m = v.value();
            m.symbol == *symbol && m.venue == venue && m.effective.contains(date)
        }))
    }
}
