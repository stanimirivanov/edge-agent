//! Whole-series acceptance; acquisition authority and latest-request selection stay outside.
mod selection;

use crate::{
    CalendarDay, CanonicalObservation, CorporateAction, DateRange, EvidenceInterval, InstrumentId,
    InstrumentMapping, ListingId, ProviderCapability, ReferenceHistory, ReferenceVersion,
    SessionDate, SessionHours, Symbol, TransformationRevision, UsdPrice, UtcInstant, Venue,
};

/// Payload-free daily evidence rejection. No partial series accompanies failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DailyQualityError {
    /// Invalid policy, range, source semantics or bounded input census.
    InvalidScope,
    /// Missing, ambiguous, expired or future-known reference/coverage.
    Reference,
    /// An expected bar is absent, cancelled, duplicated under distinct IDs or conflicting.
    Completeness,
    /// A provider bucket contradicts its explicit schedule or is not yet eligible.
    Bucket,
    /// Retained revisions conflict, omit a predecessor or contradict receipt order.
    Revision,
    /// Known actions require unsupported identity or price handling.
    UnsupportedAction,
    /// A consecutive raw close exceeds the explicit policy threshold.
    Outlier,
    /// Comparable independent sources disagree beyond the explicit policy threshold.
    SourceDisagreement,
}
impl std::fmt::Display for DailyQualityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "daily quality rejected: {self:?}")
    }
}
impl std::error::Error for DailyQualityError {}

/// Externally verified action-catalog completeness, not an inference from an empty history.
/// Ingestion must authenticate authority, source bytes and coverage before constructing this.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionCoverage {
    instrument: InstrumentId,
    range: DateRange,
    expires: UtcInstant,
}
impl ActionCoverage {
    /// Record explicit complete effective-date coverage and exclusive validity deadline.
    /// Knowledge time/source/checksum belong to the surrounding `ReferenceVersion`.
    #[must_use]
    pub const fn new(instrument: InstrumentId, range: DateRange, expires: UtcInstant) -> Self {
        Self {
            instrument,
            range,
            expires,
        }
    }
}

/// Explicit daily quality configuration; basis-point boundaries are inclusive.
/// Zero means exact equality. Splits never silently waive raw-price discontinuities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DailyQualityPolicy {
    revision: TransformationRevision,
    reference_age_ms: i64,
    close_change_bps: u16,
    source_difference_bps: u16,
}
impl DailyQualityPolicy {
    /// Exact retained revision, maximum reference age in milliseconds and both thresholds.
    #[must_use]
    pub const fn parameters(self) -> (TransformationRevision, i64, u16, u16) {
        (
            self.revision,
            self.reference_age_ms,
            self.close_change_bps,
            self.source_difference_bps,
        )
    }
    /// Require a positive maximum reference age; thresholds have no defaults.
    ///
    /// # Errors
    /// Returns `InvalidScope` for a zero age or one outside signed milliseconds.
    pub fn new(
        revision: TransformationRevision,
        reference_age_ms: u64,
        close_change_bps: u16,
        source_difference_bps: u16,
    ) -> Result<Self, DailyQualityError> {
        let reference_age_ms = i64::try_from(reference_age_ms)
            .ok()
            .filter(|age| *age > 0)
            .ok_or(DailyQualityError::InvalidScope)?;
        Ok(Self {
            revision,
            reference_age_ms,
            close_change_bps,
            source_difference_bps,
        })
    }

    /// Accept every requested open day or fail atomically. Caller supplies the exact
    /// requested range, including the latest eligible session for a latest-data request.
    /// This function does not infer global latest coverage from a truncated range.
    /// Optional comparison must cover the same range under identical source semantics.
    ///
    /// # Errors
    /// Returns a typed rejection for incomplete, stale, conflicting or unsupported evidence.
    pub fn check<'a>(
        &self,
        spec: &DailySeriesSpec<'_>,
        references: DailyReferences<'a>,
        observations: &'a [CanonicalObservation],
        comparison: Option<(ProviderCapability, &'a [CanonicalObservation])>,
    ) -> Result<CheckedDailySeries<'a>, DailyQualityError> {
        let dates = selection::dates(spec.range)?;
        if spec.buckets.len() > 252
            || spec.provider.declaration().0.shape().0
                != crate::EvidenceKind::RawBar(crate::BarCoverage::ProviderDay)
        {
            return Err(DailyQualityError::InvalidScope);
        }
        if matches!(spec.provider.declaration().0.shape().1, crate::MarketScope::Venue(v) if v != spec.venue)
        {
            return Err(DailyQualityError::InvalidScope);
        }
        let coverage = references.action_coverage;
        self.fresh(coverage, spec.cutoff)?;
        let c = coverage.value();
        let (start, end) = spec.range.bounds();
        let (covered_start, covered_end) = c.range.bounds();
        if c.instrument != spec.instrument
            || start < covered_start
            || end > covered_end
            || spec.cutoff >= c.expires
        {
            return Err(DailyQualityError::Reference);
        }
        let primary = selection::select(spec, spec.provider, observations)?;
        let secondary = comparison
            .map(|(provider, values)| {
                if provider.source() == spec.provider.source()
                    || provider.declaration().0 != spec.provider.declaration().0
                {
                    return Err(DailyQualityError::InvalidScope);
                }
                selection::select(spec, provider, values)
            })
            .transpose()?;
        let mut accepted = Vec::new();
        let mut calendars = Vec::new();
        let mut previous_close = None;
        let mut previous_end = None;
        for date in dates {
            let calendar = references
                .calendar
                .resolve_day(spec.venue, date, spec.cutoff)
                .map_err(|_| DailyQualityError::Reference)?;
            self.fresh(calendar, spec.cutoff)?;
            calendars.push(calendar);
            let actions = references
                .actions
                .actions_on(spec.instrument, date, spec.cutoff)
                .map_err(|_| DailyQualityError::UnsupportedAction)?;
            for action in &actions {
                self.fresh(action, spec.cutoff)?;
            }
            if calendar.value().hours() == SessionHours::Closed {
                // Do not drop a weekend reorganization or silently move its effective date.
                if !actions.is_empty() {
                    return Err(DailyQualityError::UnsupportedAction);
                }
                continue;
            }
            let mapping = references
                .mappings
                .resolve_mapping(spec.symbol, spec.venue, date, spec.cutoff)
                .map_err(|_| DailyQualityError::Reference)?;
            self.fresh(mapping, spec.cutoff)?;
            if mapping.value().identities() != (spec.instrument, spec.listing) {
                return Err(DailyQualityError::Reference);
            }
            let buckets: Vec<_> = spec.buckets.iter().filter(|(d, _)| *d == date).collect();
            let [(_, bucket)] = buckets.as_slice() else {
                return Err(DailyQualityError::Bucket);
            };
            let (bucket_start, bucket_end) = bucket.bounds();
            let (SessionHours::Regular(window) | SessionHours::EarlyClose(window)) =
                calendar.value().hours()
            else {
                return Err(DailyQualityError::Reference);
            };
            let (open, close) = window.bounds();
            // Core hours must fit, but never determine the provider-day endpoint.
            if bucket_start > crate::PreciseInstant::from_millis(open)
                || bucket_end < crate::PreciseInstant::from_millis(close)
                || previous_end.is_some_and(|end| bucket_start < end)
                || bucket_end.submillis() != 0
                || bucket_start.submillis() != 0
                || spec.cutoff.unix_millis() - bucket_end.millisecond_floor().unix_millis()
                    < 43_200_000
            {
                return Err(DailyQualityError::Bucket);
            }
            previous_end = Some(bucket_end);
            let observation = primary
                .get(&date)
                .copied()
                .ok_or(DailyQualityError::Completeness)?;
            let bar = selection::bar(observation, *bucket)?;
            let [_, _, _, close] = bar.values().0;
            if previous_close
                .is_some_and(|previous| exceeds(close, previous, self.close_change_bps))
            {
                return Err(DailyQualityError::Outlier);
            }
            previous_close = Some(close);
            let compared = if let Some(secondary) = &secondary {
                let other = secondary
                    .get(&date)
                    .copied()
                    .ok_or(DailyQualityError::Completeness)?;
                let prices = selection::bar(other, *bucket)?.values().0;
                if bar
                    .values()
                    .0
                    .into_iter()
                    .zip(prices)
                    .any(|(a, b)| exceeds(a.max(b), a.min(b), self.source_difference_bps))
                {
                    return Err(DailyQualityError::SourceDisagreement);
                }
                Some(other)
            } else {
                None
            };
            accepted.push(CheckedDailyDay {
                observation,
                compared,
                mapping,
                calendar,
                actions,
            });
        }
        if accepted.is_empty()
            || accepted.len() > 252
            || accepted.len() != spec.buckets.len()
            || accepted.len() != primary.len()
            || secondary
                .as_ref()
                .is_some_and(|values| values.len() != accepted.len())
        {
            return Err(DailyQualityError::Completeness);
        }
        Ok(CheckedDailySeries {
            policy: *self,
            cutoff: spec.cutoff,
            range: spec.range,
            action_coverage: coverage,
            days: accepted,
            calendars,
        })
    }

    fn fresh<T>(
        &self,
        value: &ReferenceVersion<T>,
        cutoff: UtcInstant,
    ) -> Result<(), DailyQualityError> {
        let age = cutoff.unix_millis() - value.known_at().unix_millis();
        if age < 0 || age > self.reference_age_ms {
            return Err(DailyQualityError::Reference);
        }
        Ok(())
    }
}

/// Fully explicit request. Public fields are configuration, not checked acceptance.
#[derive(Debug)]
pub struct DailySeriesSpec<'a> {
    /// Stable requested instrument.
    pub instrument: InstrumentId,
    /// Stable listing; ticker reuse cannot bridge identities.
    pub listing: ListingId,
    /// Dated provider alias resolved through retained mapping history.
    pub symbol: &'a Symbol,
    /// Listing venue, never inferred from source priority.
    pub venue: Venue,
    /// Explicit half-open requested dates, at most 366 calendar days.
    pub range: DateRange,
    /// Inclusive knowledge and availability cutoff; no wall-clock reads.
    pub cutoff: UtcInstant,
    /// Exact qualified source semantics; declaration alone does not grant rights.
    pub provider: ProviderCapability,
    /// One explicit provider-day interval per requested open session, at most 252.
    pub buckets: &'a [(SessionDate, EvidenceInterval)],
}
/// Borrowed retained reference inputs. Completeness and authority are externally verified.
#[derive(Clone, Copy, Debug)]
pub struct DailyReferences<'a> {
    /// Point-in-time dated alias mappings.
    pub mappings: &'a ReferenceHistory<InstrumentMapping>,
    /// Every calendar day, including explicitly closed dates.
    pub calendar: &'a ReferenceHistory<CalendarDay>,
    /// Complete retained action versions, including cancellations/unsupported actions.
    pub actions: &'a ReferenceHistory<CorporateAction>,
    /// Source-verified completeness attestation, independently known and expiring.
    pub action_coverage: &'a ReferenceVersion<ActionCoverage>,
}
/// Accepted observation, optional comparator, mapping, calendar and action lineage.
#[derive(Debug)]
pub struct CheckedDailyDay<'a> {
    /// Original unadjusted observation, with complete observation provenance.
    pub observation: &'a CanonicalObservation,
    /// Independent source used for price comparison, when requested.
    pub compared: Option<&'a CanonicalObservation>,
    /// Selected point-in-time mapping version.
    pub mapping: &'a ReferenceVersion<InstrumentMapping>,
    /// Selected point-in-time calendar version.
    pub calendar: &'a ReferenceVersion<CalendarDay>,
    /// Selected supported annotations; never applied to raw prices.
    pub actions: Vec<&'a ReferenceVersion<CorporateAction>>,
}
/// Complete borrowed selection; raw values stay unchanged and histories stay owned by the caller.
#[derive(Debug)]
pub struct CheckedDailySeries<'a> {
    policy: DailyQualityPolicy,
    cutoff: UtcInstant,
    range: DateRange,
    action_coverage: &'a ReferenceVersion<ActionCoverage>,
    days: Vec<CheckedDailyDay<'a>>,
    calendars: Vec<&'a ReferenceVersion<CalendarDay>>,
}
impl<'a> CheckedDailySeries<'a> {
    /// Exact complete selected calendar coverage, including explicitly closed dates.
    #[must_use]
    pub fn calendars(&self) -> &[&'a ReferenceVersion<CalendarDay>] {
        &self.calendars
    }
    /// Exact requested half-open coverage, including explicitly closed dates.
    #[must_use]
    pub const fn range(&self) -> DateRange {
        self.range
    }
    /// Stable date-ordered evidence and exact selected reference/action versions.
    #[must_use]
    pub fn days(&self) -> &[CheckedDailyDay<'a>] {
        &self.days
    }
    /// Versioned quality decision, cutoff and action-completeness provenance.
    #[must_use]
    pub const fn provenance(
        &self,
    ) -> (
        DailyQualityPolicy,
        UtcInstant,
        &'a ReferenceVersion<ActionCoverage>,
    ) {
        (self.policy, self.cutoff, self.action_coverage)
    }
}

fn exceeds(a: UsdPrice, b: UsdPrice, bps: u16) -> bool {
    let scaled = |price: UsdPrice| {
        let (coefficient, scale) = price.decimal_parts();
        coefficient.unsigned_abs() * 10_u128.pow(8 - scale)
    };
    let (a, b) = (scaled(a), scaled(b));
    // Values are <10^36. Divide before multiplying: even u16::MAX fits u128.
    // Integral difference > floor(threshold) is the exact rational comparison.
    let limit = (b / 10_000) * u128::from(bps) + ((b % 10_000) * u128::from(bps)) / 10_000;
    a.abs_diff(b) > limit
}
