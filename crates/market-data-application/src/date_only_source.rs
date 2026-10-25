//! Vendor-neutral native daily schedule and capture acceptance, with no invented event time.
use crate::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::*;
use std::collections::BTreeMap;

/// Complete owner-reviewed calendar and provider-bucket schedule for one selected request.
/// Authentication and source-byte verification remain owner obligations, not provider claims.
#[derive(Clone)]
pub struct DateOnlySchedule {
    request: AcquisitionRequest,
    mapping: ReferenceVersion<InstrumentMapping>,
    selection: EvidenceDigest,
    days: BTreeMap<
        SessionDate,
        (
            ReferenceVersion<DateOnlyBucket>,
            ReferenceVersion<CalendarDay>,
        ),
    >,
}
impl std::fmt::Debug for DateOnlySchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DateOnlySchedule(..)")
    }
}
impl DateOnlySchedule {
    /// Bind complete explicit dated calendars (including closed days) and one bucket per open day.
    /// # Errors
    /// Rejects non-native/capture requests, incomplete/duplicate/future-known references,
    /// mismatched listing/semantics, unsupported scope or an uncompleted provider day.
    pub fn new(
        selected: &SelectedRequest<'_>,
        calendars: &[ReferenceVersion<CalendarDay>],
        buckets: &[ReferenceVersion<DateOnlyBucket>],
    ) -> Result<Self, AcquisitionError> {
        let r = selected.request();
        let route = selected.route();
        let mapping = route.mapping();
        if r.contract.rules().1 != TimestampPrecision::DateOnly
            || r.contract.shape()
                != (
                    EvidenceKind::RawBar(BarCoverage::ProviderDay),
                    MarketScope::Consolidated,
                )
            || r.knowledge != KnowledgeMode::Capture
            || calendars.len() as u64 != r.range.calendar_days()
            || calendars.len() > 366
            || buckets.len() > 252
            || mapping.known_at() > r.admitted_at
        {
            return Err(AcquisitionError::Conformance);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut days = BTreeMap::new();
        for calendar in calendars {
            let (venue, date) = calendar.value().key();
            if !seen.insert(date)
                || !r.range.contains(date)
                || venue != mapping.value().binding().1
                || calendar.known_at() > r.admitted_at
            {
                return Err(AcquisitionError::Conformance);
            }
            if calendar.value().hours() == SessionHours::Closed {
                continue;
            }
            let mut matches = buckets.iter().filter(|b| b.value().declaration().0 == date);
            let bucket = matches.next().ok_or(AcquisitionError::Conformance)?;
            if matches.next().is_some() || bucket.known_at() > r.admitted_at {
                return Err(AcquisitionError::Conformance);
            }
            DateOnlyTime::new(
                date,
                bucket.clone(),
                calendar.clone(),
                PreciseInstant::from_millis(r.admitted_at),
                r.admitted_at,
            )
            .map_err(|_| AcquisitionError::Conformance)?;
            days.insert(date, (bucket.clone(), calendar.clone()));
        }
        if days.is_empty() || days.len() != buckets.len() {
            return Err(AcquisitionError::Conformance);
        }
        let mut configuration = selected
            .binding_bytes()
            .map_err(|_| AcquisitionError::Conformance)?;
        // Include complete calendar values, including closed days/core windows, not
        // just checksums: accidentally reused source witnesses must not hide edits.
        let mut ordered = calendars.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|c| c.value().key().1);
        for calendar in ordered {
            configuration.extend_from_slice(&calendar.id().get().to_be_bytes());
            configuration.extend_from_slice(&calendar.revision().get().to_be_bytes());
            configuration.extend_from_slice(&calendar.source().get().to_be_bytes());
            configuration.extend_from_slice(&calendar.known_at().unix_millis().to_be_bytes());
            configuration.extend_from_slice(calendar.checksum());
            let (y, m, d) = calendar.value().key().1.ymd();
            configuration.extend_from_slice(&y.to_be_bytes());
            configuration.extend_from_slice(&m.to_be_bytes());
            configuration.extend_from_slice(&d.to_be_bytes());
            let zone = calendar.value().timezone().1.as_bytes();
            configuration.extend_from_slice(&(zone.len() as u64).to_be_bytes());
            configuration.extend_from_slice(zone);
            let (kind, window) = match calendar.value().hours() {
                SessionHours::Closed => (0, None),
                SessionHours::Regular(w) => (1, Some(w)),
                SessionHours::EarlyClose(w) => (2, Some(w)),
                _ => return Err(AcquisitionError::Conformance),
            };
            configuration.push(kind);
            if let Some(window) = window {
                let (open, close) = window.bounds();
                configuration.extend_from_slice(&open.unix_millis().to_be_bytes());
                configuration.extend_from_slice(&close.unix_millis().to_be_bytes());
            }
        }
        for (date, (bucket, calendar)) in &days {
            let time = DateOnlyTime::new(
                *date,
                bucket.clone(),
                calendar.clone(),
                PreciseInstant::from_millis(r.admitted_at),
                r.admitted_at,
            )
            .map_err(|_| AcquisitionError::Conformance)?;
            configuration.extend_from_slice(
                freeze_date_only_time(&time)
                    .map_err(|_| AcquisitionError::Conformance)?
                    .bytes(),
            );
        }
        Ok(Self {
            request: *r,
            mapping: mapping.clone(),
            days,
            selection: DailyEvidenceArtifact::hash(&configuration),
        })
    }
    /// Exact immutable request; not a fresh authorization grant.
    #[must_use]
    pub const fn request(&self) -> &AcquisitionRequest {
        &self.request
    }
    /// Reviewed alias/listing witness used for outbound binding.
    #[must_use]
    pub const fn mapping(&self) -> &ReferenceVersion<InstrumentMapping> {
        &self.mapping
    }
    /// Exact owner configuration binding required on the durable native admission.
    #[must_use]
    pub const fn configuration_digest(&self) -> &EvidenceDigest {
        &self.selection
    }
    /// Check every explicit bucket satisfies current reviewed availability age.
    #[must_use]
    pub fn satisfies_embargo(&self, at: UtcInstant, embargo_ms: u32) -> bool {
        self.days.values().all(|(b, _)| {
            let (seconds, nanos) = b.value().declaration().1.bounds().1.unix_parts();
            let end = i128::from(seconds) * 1000 + i128::from(nanos / 1_000_000);
            i128::from(at.unix_millis()) - end >= i128::from(embargo_ms)
        })
    }
    /// Ascending expected open labels; absent rows are failure, never zero prices.
    pub fn dates(&self) -> impl ExactSizeIterator<Item = SessionDate> + '_ {
        self.days.keys().copied()
    }
    /// Validate a response against the immutable admission, exact complete schedule and actual receipt.
    /// No quality/publishing grant follows from a captured object.
    /// # Errors
    /// Rejects immutable admission/scope changes, incomplete/reordered rows or invalid values.
    /// The caller must separately recheck current rights and embargo at release.
    pub fn capture(
        &self,
        record: &AdmissionRecord,
        rows: &[(SessionDate, [UsdPrice; 4], ShareVolume)],
        response_sha256: EvidenceDigest,
        received: UtcInstant,
        derived: UtcInstant,
    ) -> Result<DateOnlyDailyArtifact, AcquisitionError> {
        fn bad<T>(_: T) -> AcquisitionError {
            AcquisitionError::Response
        }
        if record.request() != &self.request
            || record.configuration() != Some(self.selection.as_str())
            || rows.len() != self.days.len()
            || received < self.request.admitted_at
            || derived >= self.request.deadline
            || self.mapping.value().identities().1 != record.listing().map_err(bad)?
        {
            return Err(AcquisitionError::Response);
        }
        let mut result = Vec::with_capacity(rows.len());
        for ((label, ohlc, volume), (date, (bucket, calendar))) in rows.iter().zip(&self.days) {
            if label != date {
                return Err(AcquisitionError::Response);
            }
            RawBar::new(
                bucket.value().declaration().1,
                BarCoverage::ProviderDay,
                *ohlc,
                *volume,
            )
            .map_err(bad)?;
            let time = DateOnlyTime::new(
                *date,
                bucket.clone(),
                calendar.clone(),
                PreciseInstant::from_millis(received),
                derived,
            )
            .map_err(bad)?;
            let decimal = |parts: (i128, u32)| -> Result<EvidenceDecimal, AcquisitionError> {
                Ok(EvidenceDecimal {
                    coefficient: parts.0.to_string(),
                    scale: u8::try_from(parts.1).map_err(bad)?,
                })
            };
            result.push(DateOnlyDailyRow {
                time: freeze_date_only_time(&time).map_err(bad)?.data().clone(),
                ohlc: [
                    decimal(ohlc[0].decimal_parts())?,
                    decimal(ohlc[1].decimal_parts())?,
                    decimal(ohlc[2].decimal_parts())?,
                    decimal(ohlc[3].decimal_parts())?,
                ],
                volume: decimal(volume.decimal_parts())?,
            });
        }
        let id = |n: u128| EvidenceId::new(&format!("{n:032x}")).map_err(bad);
        let r = self.request;
        let semantics = r.contract.rules().0.version();
        let date = |d: SessionDate| {
            let (y, m, d) = d.ymd();
            EvidenceDate::new(&format!("{y:04}-{m:02}-{d:02}")).map_err(bad)
        };
        DateOnlyDailyArtifact::new(DateOnlyDailyCapture {
            version: DateOnlyDailyFormat::V1,
            scope: EvidenceScope {
                request: record.command().request.clone(),
                request_sha256: record.command().request_sha256.clone(),
                principal: id(r.scope.principal.get())?,
                account: id(r.scope.account.get())?,
                source: id(r.scope.source.get())?,
                instrument: id(r.scope.instrument.get())?,
                listing: id(self.mapping.value().identities().1.get())?,
                audience: id(r.scope.audience.get())?,
                storage: id(r.scope.storage.get())?,
                endpoint: id(r.scope.endpoint.get())?,
            },
            semantics: EvidenceVersion {
                id: id(semantics.0.get())?,
                revision: EvidenceRevision::new(&semantics.1.get().to_string()).map_err(bad)?,
            },
            selection_sha256: self.selection.clone(),
            response_sha256,
            range: [date(r.range.bounds().0)?, date(r.range.bounds().1)?],
            rows: result,
        })
        .map_err(bad)
    }
}
