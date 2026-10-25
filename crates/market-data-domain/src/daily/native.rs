//! Whole native snapshots, not a conversion into precise source observations.
use super::*;
use crate::{DateOnlyTime, ObservationContext, ObservationVersion, RawBar, TimestampPrecision};

/// Owner-versioned native row. Bucket metadata never becomes an event timestamp.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeDailyObservation {
    context: ObservationContext,
    version: ObservationVersion,
    time: DateOnlyTime,
    bar: RawBar,
}
impl std::fmt::Debug for NativeDailyObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeDailyObservation(..)")
    }
}
impl NativeDailyObservation {
    /// Bind exact native temporal evidence, explicit lineage and raw values.
    /// # Errors
    /// Rejects foreign dates, precise contracts, conditions or mismatched buckets.
    pub fn new(
        context: ObservationContext,
        version: ObservationVersion,
        time: DateOnlyTime,
        bar: RawBar,
    ) -> Result<Self, DailyQualityError> {
        let contract = context.origin().1.declaration().0;
        if contract.rules().1 != TimestampPrecision::DateOnly
            || context.origin().1.declaration().1 != crate::DeliveryCapability::Snapshots
            || context.origin().1.declaration().2 != crate::KnowledgeCapability::CaptureOnly
            || contract.shape().0 != crate::EvidenceKind::RawBar(crate::BarCoverage::ProviderDay)
            || context.session_conditions() != (time.source_date(), &[][..])
            || bar.bucket()
                != (
                    time.provenance().0.value().declaration().1,
                    crate::BarCoverage::ProviderDay,
                )
        {
            return Err(DailyQualityError::InvalidScope);
        }
        Ok(Self {
            context,
            version,
            time,
            bar,
        })
    }
    /// Exact supplied instrument, provider and transformation context.
    #[must_use]
    pub const fn context(&self) -> &ObservationContext {
        &self.context
    }
    /// Owner-assigned identity/revision; no identity is inferred from a price or date.
    #[must_use]
    pub const fn version(&self) -> ObservationVersion {
        self.version
    }
    /// Native label and original receipt/derivation with reviewed bucket provenance.
    #[must_use]
    pub const fn time(&self) -> &DateOnlyTime {
        &self.time
    }
    /// Unadjusted original prices and shares.
    #[must_use]
    pub const fn bar(&self) -> RawBar {
        self.bar
    }
}
/// A selected native row and exact point-in-time reference records.
#[derive(Debug)]
pub struct CheckedNativeDailyDay<'a> {
    /// Original native observation, not an invented precise event.
    pub observation: &'a NativeDailyObservation,
    /// Exact selected mapping.
    pub mapping: &'a ReferenceVersion<InstrumentMapping>,
    /// Exact selected dated calendar, matching temporal provenance.
    pub calendar: &'a ReferenceVersion<CalendarDay>,
    /// Supported raw-price annotations, never adjustments or outlier waivers.
    pub actions: Vec<&'a ReferenceVersion<CorporateAction>>,
}
/// Complete native quality result, not an authority or publication token.
#[derive(Debug)]
pub struct CheckedNativeDailySeries<'a> {
    days: Vec<CheckedNativeDailyDay<'a>>,
    calendars: Vec<&'a ReferenceVersion<CalendarDay>>,
}
impl<'a> CheckedNativeDailySeries<'a> {
    /// Complete ascending accepted open days.
    #[must_use]
    pub fn days(&self) -> &[CheckedNativeDailyDay<'a>] {
        &self.days
    }
    /// Complete requested calendar coverage, including explicitly closed days.
    #[must_use]
    pub fn calendars(&self) -> &[&'a ReferenceVersion<CalendarDay>] {
        &self.calendars
    }
}
impl DailyQualityPolicy {
    /// Accept an entire selected native snapshot under the existing daily profile.
    /// No source comparison is claimed. Lineage continuity against prior retained
    /// snapshots is checked separately by the producer, not inferred from IDs here.
    /// # Errors
    /// Rejects incomplete/foreign snapshots, references, buckets, actions or outliers.
    pub fn check_native<'a>(
        &self,
        spec: &DailySeriesSpec<'_>,
        references: DailyReferences<'a>,
        observations: &'a [NativeDailyObservation],
    ) -> Result<CheckedNativeDailySeries<'a>, DailyQualityError> {
        use DailyQualityError as E;
        let contract = spec.provider.declaration().0;
        if contract.rules().1 != TimestampPrecision::DateOnly
            || contract.shape()
                != (
                    crate::EvidenceKind::RawBar(crate::BarCoverage::ProviderDay),
                    crate::MarketScope::Consolidated,
                )
            || observations.is_empty()
            || observations.len() > 252
            || spec.buckets.len() > 252
        {
            return Err(E::InvalidScope);
        }
        self.fresh(references.action_coverage, spec.cutoff)?;
        let c = references.action_coverage.value();
        let (start, end) = spec.range.bounds();
        let (a, b) = c.range.bounds();
        if c.instrument != spec.instrument || start < a || end > b || spec.cutoff >= c.expires {
            return Err(E::Reference);
        }
        let mut rows = std::collections::BTreeMap::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut previous_date = None;
        for o in observations {
            let date = o.time.source_date();
            let (instrument, provider, _) = o.context.origin();
            if instrument != spec.instrument
                || provider != spec.provider
                || !spec.range.contains(date)
                || o.time.local_times().1 > spec.cutoff
            {
                return Err(E::InvalidScope);
            }
            if previous_date.is_some_and(|p| p >= date) || !ids.insert(o.version.lineage().0) {
                return Err(E::Revision);
            }
            previous_date = Some(date);
            rows.insert(date, o);
        }
        let mut days = Vec::new();
        let mut calendars = Vec::new();
        let mut previous_end = None;
        let mut previous_close = None;
        for date in selection::dates(spec.range)? {
            let calendar = references
                .calendar
                .resolve_day(spec.venue, date, spec.cutoff)
                .map_err(|_| E::Reference)?;
            self.fresh(calendar, spec.cutoff)?;
            calendars.push(calendar);
            let actions = references
                .actions
                .actions_on(spec.instrument, date, spec.cutoff)
                .map_err(|_| E::UnsupportedAction)?;
            for action in &actions {
                self.fresh(action, spec.cutoff)?;
            }
            if calendar.value().hours() == SessionHours::Closed {
                if !actions.is_empty() {
                    return Err(E::UnsupportedAction);
                }
                if rows.contains_key(&date) {
                    return Err(E::Completeness);
                }
                continue;
            }
            let mapping = references
                .mappings
                .resolve_mapping(spec.symbol, spec.venue, date, spec.cutoff)
                .map_err(|_| E::Reference)?;
            self.fresh(mapping, spec.cutoff)?;
            if mapping.value().identities() != (spec.instrument, spec.listing) {
                return Err(E::Reference);
            }
            let o = rows.get(&date).copied().ok_or(E::Completeness)?;
            let (rule, pinned) = o.time.provenance();
            self.fresh(rule, spec.cutoff)?;
            if pinned != calendar {
                return Err(E::Reference);
            }
            let bucket = rule.value().declaration().1;
            let mut matches = spec.buckets.iter().filter(|(d, _)| *d == date);
            if matches.next().map(|(_, b)| *b) != Some(bucket) || matches.next().is_some() {
                return Err(E::Bucket);
            }
            let (begin, end) = bucket.bounds();
            if previous_end.is_some_and(|p| begin < p)
                || spec.cutoff.unix_millis() - end.millisecond_floor().unix_millis() < 43_200_000
            {
                return Err(E::Bucket);
            }
            previous_end = Some(end);
            let close = o.bar.values().0[3];
            if previous_close.is_some_and(|p| exceeds(close, p, self.close_change_bps)) {
                return Err(E::Outlier);
            }
            previous_close = Some(close);
            days.push(CheckedNativeDailyDay {
                observation: o,
                mapping,
                calendar,
                actions,
            });
        }
        if days.len() != observations.len() || days.len() != spec.buckets.len() {
            return Err(E::Completeness);
        }
        Ok(CheckedNativeDailySeries { days, calendars })
    }
}
