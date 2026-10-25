//! Complete precise-time source inputs; never promotes dates into provider instants.
use crate::*;
use edgeagent_contracts::evidence::{DailyEvidenceArtifact, EvidenceDigest};
use edgeagent_market_data_domain::*;
use std::collections::{BTreeMap, BTreeSet};
mod encoding;

/// Explicit service-owned observation identity and reviewed bucket for an open day.
#[derive(Clone, Copy)]
pub struct DailyObservationSlot {
    /// Reviewed exchange-local date, not inferred from a UTC host timezone.
    pub date: SessionDate,
    /// Explicit provider-day coverage, not assumed to equal core hours.
    pub bucket: EvidenceInterval,
    /// Owner-assigned immutable observation lineage; never synthesized from prices.
    pub version: ObservationVersion,
}
/// Untrusted precise source row, mapped only after complete schedule validation.
#[derive(Clone, Copy)]
pub struct PreciseDailyRow {
    /// Actual source-reported opening timestamp, with no rounding.
    pub event_at: PreciseInstant,
    /// Exact raw USD/share open/high/low/close.
    pub ohlc: [UsdPrice; 4],
    /// Exact reported shares.
    pub volume: ShareVolume,
    /// Actual complete local body receipt for this row's page.
    pub received_at: UtcInstant,
}
/// Source-byte provenance for one confirmed page. Raw retention needs separate rights.
#[derive(Clone)]
pub struct DailyPageReceipt {
    /// SHA-256 over exact response bytes, not a reserialized DTO.
    pub body_sha256: EvidenceDigest,
    /// Original complete local receipt; accounting recovery must not replace it.
    pub received_at: UtcInstant,
}
/// Unapproved complete source capture; no wire/storage/publication conversion is implied.
#[derive(Clone)]
pub struct DailySourceCapture {
    batch: ProviderBatch,
    receipts: Vec<DailyPageReceipt>,
}
impl DailySourceCapture {
    /// Canonical inputs still requiring current `validate_batch` and whole-series quality.
    #[must_use]
    pub const fn batch(&self) -> &ProviderBatch {
        &self.batch
    }
    /// Ordered confirmed response provenance, not a retained-read grant.
    #[must_use]
    pub fn receipts(&self) -> &[DailyPageReceipt] {
        &self.receipts
    }
}
/// Frozen selected request, complete calendar and explicit observation/bucket configuration.
#[derive(Clone)]
pub struct PreciseDailySchedule {
    request: AcquisitionRequest,
    mapping: ReferenceVersion<InstrumentMapping>,
    slots: Vec<DailyObservationSlot>,
    transformation: TransformationRevision,
    digest: EvidenceDigest,
}
macro_rules! redact { ($($t:ty),*) => {$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*}; }
redact!(
    DailyObservationSlot,
    PreciseDailyRow,
    DailyPageReceipt,
    DailySourceCapture,
    PreciseDailySchedule
);
impl PreciseDailySchedule {
    /// Validate a complete explicit capture-only consolidated daily plan before external work.
    /// Source timestamps must match supplied bucket starts; all calendars must be known now.
    /// # Errors
    /// Rejects missing/duplicate/foreign/future references, incomplete or overlapping buckets,
    /// non-nanosecond contracts, duplicate observation IDs and unsupported sessions.
    pub fn new(
        selected: &SelectedRequest<'_>,
        calendars: &[ReferenceVersion<CalendarDay>],
        slots: &[DailyObservationSlot],
        transformation: TransformationRevision,
    ) -> Result<Self, AcquisitionError> {
        let r = selected.request();
        let route = selected.route();
        let mapping = route.mapping();
        if r.knowledge != KnowledgeMode::Capture
            || r.contract.rules().1 != TimestampPrecision::Nanoseconds
            || r.contract.shape()
                != (
                    EvidenceKind::RawBar(BarCoverage::ProviderDay),
                    MarketScope::Consolidated,
                )
            || calendars.len() as u64 != r.range.calendar_days()
            || calendars.len() > 366
            || slots.is_empty()
            || slots.len() > 252
            || mapping.known_at() > r.admitted_at
        {
            return Err(AcquisitionError::Conformance);
        }
        let mut dates = BTreeMap::new();
        for c in calendars {
            let (venue, date) = c.value().key();
            if c.known_at() > r.admitted_at
                || venue != mapping.value().binding().1
                || !r.range.contains(date)
                || dates.insert(date, c).is_some()
            {
                return Err(AcquisitionError::Conformance);
            }
        }
        let mut expected = Vec::new();
        for (date, c) in &dates {
            match c.value().hours() {
                SessionHours::Closed => {}
                SessionHours::Regular(w) | SessionHours::EarlyClose(w) => expected.push((*date, w)),
                _ => return Err(AcquisitionError::Conformance),
            }
        }
        if expected.len() != slots.len() {
            return Err(AcquisitionError::Conformance);
        }
        let mut ids = BTreeSet::new();
        let mut previous = None;
        for (slot, (date, window)) in slots.iter().zip(expected) {
            let (begin, end) = slot.bucket.bounds();
            let (open, close) = window.bounds();
            let (seconds, nanos) = begin.unix_parts();
            let (end_seconds, end_nanos) = end.unix_parts();
            let duration = (i128::from(end_seconds) - i128::from(seconds)) * 1_000_000_000
                + i128::from(end_nanos)
                - i128::from(nanos);
            if slot.date != date
                || !ids.insert(slot.version.lineage().0.get())
                || begin > PreciseInstant::from_millis(open)
                || end < PreciseInstant::from_millis(close)
                || end > PreciseInstant::from_millis(r.admitted_at)
                || duration > 93_600_000_000_000
                || previous.is_some_and(|p| begin < p)
            {
                return Err(AcquisitionError::Conformance);
            }
            previous = Some(end);
        }
        // Canonical primitives include actual values as well as witnesses, so reused
        // source checksums cannot hide changed calendars, IDs or coverage.
        let bytes = encoding::binding(selected, dates, slots, transformation)?;
        Ok(Self {
            request: *r,
            mapping: mapping.clone(),
            slots: slots.to_vec(),
            transformation,
            digest: DailyEvidenceArtifact::hash(&bytes),
        })
    }
    /// Exact original request, not renewable authority.
    #[must_use]
    pub const fn request(&self) -> &AcquisitionRequest {
        &self.request
    }
    /// Selected listing and alias witness; source responses cannot replace it.
    #[must_use]
    pub const fn mapping(&self) -> &ReferenceVersion<InstrumentMapping> {
        &self.mapping
    }
    /// Exact immutable owner configuration to pin in admission-v2.
    #[must_use]
    pub const fn configuration_digest(&self) -> &EvidenceDigest {
        &self.digest
    }
    /// Complete ascending expected source opening instants and owner observation versions.
    #[must_use]
    pub fn slots(&self) -> &[DailyObservationSlot] {
        &self.slots
    }
    /// Require every completed bucket to satisfy an explicit minimum age.
    #[must_use]
    pub fn satisfies_embargo(&self, at: UtcInstant, ms: u32) -> bool {
        self.slots.iter().all(|s| {
            let (sec, nanos) = s.bucket.bounds().1.unix_parts();
            i128::from(at.unix_millis()) * 1_000_000
                - (i128::from(sec) * 1_000_000_000 + i128::from(nanos))
                >= i128::from(ms) * 1_000_000
        })
    }
    /// Map a complete capture without fabricating timestamps or historical knowledge.
    /// The caller separately rechecks current rights/retention before releasing data.
    /// # Errors
    /// Rejects foreign admissions, incomplete rows/receipts, wrong times or invalid raw bars.
    pub fn capture(
        &self,
        record: &AdmissionRecord,
        provider: ProviderCapability,
        rows: &[PreciseDailyRow],
        receipts: Vec<DailyPageReceipt>,
    ) -> Result<DailySourceCapture, AcquisitionError> {
        fn bad<T>(_: T) -> AcquisitionError {
            AcquisitionError::Response
        }
        if record.request() != &self.request
            || record.configuration() != Some(self.digest.as_str())
            || record.listing().map_err(bad)? != self.mapping.value().identities().1
            || rows.len() != self.slots.len()
            || receipts.is_empty()
            || receipts.iter().any(|p| {
                p.received_at < self.request.admitted_at || p.received_at >= self.request.deadline
            })
            || receipts
                .windows(2)
                .any(|w| matches!(w,[a,b] if a.received_at>b.received_at))
            || receipts.len() > self.request.budget.pages.get() as usize
            || provider
                != ProviderCapability::new(
                    self.request.scope.source,
                    self.request.contract,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                )
        {
            return Err(AcquisitionError::Response);
        }
        let mut observations = Vec::with_capacity(rows.len());
        for (row, slot) in rows.iter().zip(&self.slots) {
            if row.event_at != slot.bucket.bounds().0
                || row.received_at < self.request.admitted_at
                || row.received_at >= self.request.deadline
                || !receipts.iter().any(|p| p.received_at == row.received_at)
            {
                return Err(AcquisitionError::Response);
            }
            let value = RawBar::new(slot.bucket, BarCoverage::ProviderDay, row.ohlc, row.volume)
                .map_err(bad)?;
            observations.push(
                CanonicalObservation::new(
                    ObservationContext::new(
                        self.request.scope.instrument,
                        provider,
                        self.transformation,
                        slot.date,
                        &[],
                    )
                    .map_err(bad)?,
                    slot.version,
                    EvidenceTimes::new(
                        row.event_at,
                        None,
                        None,
                        PreciseInstant::from_millis(row.received_at),
                    )
                    .map_err(bad)?,
                    None,
                    Some(ObservationValue::Bar(value)),
                )
                .map_err(bad)?,
            );
        }
        Ok(DailySourceCapture {
            batch: ProviderBatch {
                request: self.request.id,
                provider,
                observations,
            },
            receipts,
        })
    }
}
