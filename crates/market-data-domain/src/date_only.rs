//! Native source dates and reviewed bucket derivations, never fabricated source instants.
use crate::{
    CalendarDay, EvidenceInterval, ObservationError as Error, PreciseInstant, ReferenceVersion,
    SemanticReference, SessionDate, SessionHours, UtcInstant,
};

/// Owner-reviewed provider-day bounds for one label and exact calendar revision.
/// Source authority/checksum/receipt belong to the enclosing `ReferenceVersion`.
/// Bounds are supplied, not inferred from midnight or the core trading session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateOnlyBucket {
    label: SessionDate,
    interval: EvidenceInterval,
    calendar: SemanticReference,
}
impl DateOnlyBucket {
    /// Bind explicit millisecond-aligned daily bounds (at most 26 hours) to a calendar.
    /// The capacity permits supplied 23/25-hour DST days without extrapolating rules.
    /// # Errors
    /// Rejects fractional-millisecond bounds or a bucket wider than the daily ceiling.
    pub fn new(
        label: SessionDate,
        interval: EvidenceInterval,
        calendar: SemanticReference,
    ) -> Result<Self, Error> {
        let (start, end) = interval.bounds();
        if start.submillis() != 0
            || end.submillis() != 0
            || end.millisecond_floor().unix_millis() - start.millisecond_floor().unix_millis()
                > 93_600_000
        {
            return Err(Error::Timestamp);
        }
        Ok(Self {
            label,
            interval,
            calendar,
        })
    }
    /// Native local date, explicit UTC interval and selected calendar version.
    #[must_use]
    pub const fn declaration(self) -> (SessionDate, EvidenceInterval, SemanticReference) {
        (self.label, self.interval, self.calendar)
    }
}

/// Date-only temporal evidence. It deliberately has no `event_at` or conversion to
/// `EvidenceTimes`: bucket bounds are derived metadata, not provider timestamps.
/// Authenticating reviews, current rights and historical availability remains external.
///
/// ```compile_fail,E0277
/// use edgeagent_market_data_domain::{DateOnlyTime, EvidenceTimes};
/// fn cannot_invent_source_precision(value: DateOnlyTime) -> EvidenceTimes {
///     value.into()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateOnlyTime {
    label: SessionDate,
    bucket: ReferenceVersion<DateOnlyBucket>,
    calendar: ReferenceVersion<CalendarDay>,
    received: PreciseInstant,
    derived: UtcInstant,
}
impl DateOnlyTime {
    /// Validate one native label against exact reviewed bucket/calendar inputs.
    /// `received` is actual local receipt; `derived` is actual local derivation time.
    /// Reviews can arrive after the payload but must be known before derivation.
    /// # Errors
    /// Rejects mismatched labels/revisions, closed days, missing core coverage,
    /// premature receipt, future-known references or backdated derivation.
    pub fn new(
        label: SessionDate,
        bucket: ReferenceVersion<DateOnlyBucket>,
        calendar: ReferenceVersion<CalendarDay>,
        received: PreciseInstant,
        derived: UtcInstant,
    ) -> Result<Self, Error> {
        let (date, interval, pin) = bucket.value().declaration();
        let (id, revision) = pin.version();
        let (begin, end) = interval.bounds();
        let window = match calendar.value().hours() {
            SessionHours::Regular(w) | SessionHours::EarlyClose(w) => w,
            SessionHours::Closed => return Err(Error::Contract),
        };
        let (open, close) = window.bounds();
        if label != date
            || label != calendar.value().key().1
            || id != calendar.id()
            || revision != calendar.revision()
        {
            return Err(Error::Contract);
        }
        if begin > PreciseInstant::from_millis(open)
            || end < PreciseInstant::from_millis(close)
            || received < end
            || PreciseInstant::from_millis(derived) < received
            || bucket.known_at() > derived
            || calendar.known_at() > derived
        {
            return Err(Error::TimeOrder);
        }
        Ok(Self {
            label,
            bucket,
            calendar,
            received,
            derived,
        })
    }
    /// Original provider date, not a UTC event or availability time.
    #[must_use]
    pub const fn source_date(&self) -> SessionDate {
        self.label
    }
    /// Retained rule and calendar records; callers retain/authenticate their source bytes.
    #[must_use]
    pub const fn provenance(
        &self,
    ) -> (
        &ReferenceVersion<DateOnlyBucket>,
        &ReferenceVersion<CalendarDay>,
    ) {
        (&self.bucket, &self.calendar)
    }
    /// Actual local receipt and derivation, preserving receipt precision independently.
    #[must_use]
    pub const fn local_times(&self) -> (PreciseInstant, UtcInstant) {
        (self.received, self.derived)
    }
}
