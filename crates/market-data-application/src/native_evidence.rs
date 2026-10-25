//! Producer acceptance only: no storage, transaction, provider or publication capability.
use crate::evidence::{date, id, millis, revision, version, witness};
use crate::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::*;
use std::collections::BTreeMap;

/// Fully explicit native quality inputs; referenced records must be authenticated by the owner.
#[derive(Debug)]
pub struct NativeEvidenceFreeze<'a> {
    /// Complete frozen schedule whose digest is pinned in admission.
    pub schedule: &'a DateOnlySchedule,
    /// Original bounded native source capture, not a quality grant.
    pub capture: &'a DateOnlyDailyArtifact,
    /// One explicit owner identity/revision per native row, in the same date order.
    pub versions: &'a [ObservationVersion],
    /// Explicit row transformation, independent from the quality-policy revision.
    pub transformation: TransformationRevision,
    /// Current complete point-in-time mappings, calendars and corporate actions.
    pub references: DailyReferences<'a>,
    /// Explicit retained quality parameters.
    pub policy: DailyQualityPolicy,
    /// Owner-authorized predecessor snapshot, never fetched or authorized by its hash.
    pub previous: Option<&'a NativeDailyArtifact>,
}
fn contract<T>(_: T) -> EvidenceFreezeError {
    EvidenceFreezeError::Contract
}
fn instant(t: EvidenceTime) -> Result<PreciseInstant, EvidenceFreezeError> {
    PreciseInstant::from_unix_parts(t.seconds, t.nanos).map_err(contract)
}
fn utc(t: EvidenceTime) -> Result<UtcInstant, EvidenceFreezeError> {
    if !t.nanos.is_multiple_of(1_000_000) {
        return Err(EvidenceFreezeError::Contract);
    }
    let ms = i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000);
    UtcInstant::from_unix_millis(i64::try_from(ms).map_err(contract)?).map_err(contract)
}
/// Recheck original admission/current authority, native quality and correction lineage, then seal bytes.
/// Capture-only: never infers past availability, precise source time, IDs or adjusted prices.
/// # Errors
/// Rejects changed authority/configuration, partial input, quality or immutable lineage conflicts.
pub fn freeze_native_evidence(
    record: &AdmissionRecord,
    admitted: &AuthorizedAcquisition<'_>,
    catalogs: AuthorityCatalogs<'_>,
    now: UtcInstant,
    cancel: &Cancellation,
    input: NativeEvidenceFreeze<'_>,
) -> Result<NativeDailyArtifact, EvidenceFreezeError> {
    let current = catalogs
        .admit(admitted.request(), now, cancel)
        .map_err(EvidenceFreezeError::Acquisition)?;
    if current.authority().0 != admitted.authority().0 || now >= admitted.delete_by() {
        return Err(EvidenceFreezeError::Acquisition(AcquisitionError::Rights));
    }
    if current.authority().1 != admitted.authority().1 {
        return Err(EvidenceFreezeError::Acquisition(
            AcquisitionError::Conformance,
        ));
    }
    let r = admitted.request();
    let schedule = input.schedule;
    let capture = input.capture.data();
    let listing = schedule.mapping().value().identities().1;
    record
        .verify(admitted, listing, input.policy)
        .map_err(contract)?;
    if schedule.request() != r
        || r.knowledge != KnowledgeMode::Capture
        || record.configuration() != Some(schedule.configuration_digest().as_str())
        || capture.selection_sha256 != *schedule.configuration_digest()
        || input.versions.len() != capture.rows.len()
        || capture.rows.len() != schedule.reviewed_days().len()
        || capture.range != [date(r.range.bounds().0)?, date(r.range.bounds().1)?]
        || !schedule.satisfies_embargo(now, current.authority().0.value().embargo_ms)
    {
        return Err(EvidenceFreezeError::Contract);
    }
    let semantics = r.contract.rules().0.version();
    let s = r.scope;
    let expected = EvidenceScope {
        request: record.command().request.clone(),
        request_sha256: record.command().request_sha256.clone(),
        principal: id(s.principal.get())?,
        account: id(s.account.get())?,
        source: id(s.source.get())?,
        instrument: id(s.instrument.get())?,
        listing: id(listing.get())?,
        audience: id(s.audience.get())?,
        storage: id(s.storage.get())?,
        endpoint: id(s.endpoint.get())?,
    };
    if capture.scope != expected
        || capture.semantics != version(semantics.0.get(), semantics.1.get())?
    {
        return Err(EvidenceFreezeError::Contract);
    }
    let mut observations = Vec::with_capacity(capture.rows.len());
    let mut buckets = Vec::with_capacity(capture.rows.len());
    for ((row, (label, (rule, calendar))), v) in capture
        .rows
        .iter()
        .zip(schedule.reviewed_days())
        .zip(input.versions)
    {
        let t = &row.time;
        if t.source_date != date(*label)?
            || t.received_at < millis(r.admitted_at)?
            || t.derived_at >= millis(r.deadline)?
        {
            return Err(EvidenceFreezeError::Contract);
        }
        let native = DateOnlyTime::new(
            *label,
            rule.clone(),
            calendar.clone(),
            instant(t.received_at)?,
            utc(t.derived_at)?,
        )
        .map_err(contract)?;
        if freeze_date_only_time(&native)?.data() != t {
            return Err(EvidenceFreezeError::Contract);
        }
        let price = |d: &EvidenceDecimal| -> Result<UsdPrice, EvidenceFreezeError> {
            UsdPrice::new(d.coefficient.parse().map_err(contract)?, u32::from(d.scale))
                .map_err(contract)
        };
        let bar = RawBar::new(
            rule.value().declaration().1,
            BarCoverage::ProviderDay,
            [
                price(&row.ohlc[0])?,
                price(&row.ohlc[1])?,
                price(&row.ohlc[2])?,
                price(&row.ohlc[3])?,
            ],
            ShareVolume::new(
                row.volume.coefficient.parse().map_err(contract)?,
                u32::from(row.volume.scale),
            )
            .map_err(contract)?,
        )
        .map_err(contract)?;
        observations.push(
            NativeDailyObservation::new(
                ObservationContext::new(
                    r.instrument(),
                    current.authority().1.value().capability,
                    input.transformation,
                    *label,
                    &[],
                )
                .map_err(contract)?,
                *v,
                native,
                bar,
            )
            .map_err(EvidenceFreezeError::Quality)?,
        );
        buckets.push((*label, bar.bucket().0));
    }
    let spec = DailySeriesSpec {
        instrument: r.instrument(),
        listing,
        symbol: schedule.mapping().value().binding().0,
        venue: schedule.mapping().value().binding().1,
        range: r.range,
        cutoff: now,
        provider: current.authority().1.value().capability,
        buckets: &buckets,
    };
    let checked = input
        .policy
        .check_native(&spec, input.references, &observations)
        .map_err(EvidenceFreezeError::Quality)?;
    if checked
        .calendars()
        .iter()
        .any(|c| !schedule.reviewed_calendars().contains(c))
    {
        return Err(EvidenceFreezeError::Contract);
    }
    let mut references = BTreeMap::new();
    let mut add = |v: EvidenceReference| -> Result<(), EvidenceFreezeError> {
        if references
            .insert((v.kind, v.version.clone()), v.clone())
            .is_some_and(|p| p != v)
        {
            return Err(EvidenceFreezeError::Contract);
        }
        Ok(())
    };
    add(witness(
        EvidenceReferenceKind::Rights,
        current.authority().0,
        None,
    )?)?;
    add(witness(
        EvidenceReferenceKind::ProviderReview,
        current.authority().1,
        None,
    )?)?;
    add(witness(
        EvidenceReferenceKind::ActionCoverage,
        input.references.action_coverage,
        None,
    )?)?;
    for c in checked.calendars() {
        add(witness(
            EvidenceReferenceKind::Calendar,
            c,
            Some(c.value().key().1),
        )?)?;
    }
    let mut lineage = Vec::new();
    for day in checked.days() {
        let (i, v, p) = day.observation.version().lineage();
        let mapping = witness(EvidenceReferenceKind::Mapping, day.mapping, None)?;
        let mut actions = Vec::new();
        for a in &day.actions {
            let a = witness(
                EvidenceReferenceKind::Action,
                a,
                Some(day.observation.time().source_date()),
            )?;
            actions.push(a.version.clone());
            add(a)?;
        }
        actions.sort();
        lineage.push(NativeDailyLineage {
            observation: version(i.get(), v.get())?,
            predecessor: p.map(|p| revision(p.get())).transpose()?,
            transformation: revision(input.transformation.get())?,
            mapping: mapping.version.clone(),
            actions,
        });
        add(mapping)?;
    }
    let (rev, age, close_bps, source_bps) = input.policy.parameters();
    let delete = now
        .unix_millis()
        .checked_add(86_400_000)
        .ok_or(EvidenceFreezeError::Contract)?
        .min(record.delete_by_ms().map_err(contract)?)
        .min(admitted.delete_by().unix_millis());
    let artifact = NativeDailyArtifact::new(NativeDailyEvidence {
        version: NativeDailyFormat::V1,
        capture: capture.clone(),
        captured_at: millis(now)?,
        delete_by: millis(UtcInstant::from_unix_millis(delete).map_err(contract)?)?,
        quality: EvidenceQuality {
            revision: revision(rev.get())?,
            reference_age_ms: EvidenceAgeMillis::new(&age.to_string())?,
            close_change_bps: close_bps,
            source_difference_bps: source_bps,
        },
        references: references.into_values().collect(),
        lineage,
        replaces: input.previous.map(|p| p.digest().clone()),
    })?;
    check_previous(&artifact, input.previous)?;
    Ok(artifact)
}
fn check_previous(
    new: &NativeDailyArtifact,
    previous: Option<&NativeDailyArtifact>,
) -> Result<(), EvidenceFreezeError> {
    let n = new.data();
    let Some(previous) = previous else {
        if n.lineage.iter().any(|l| l.predecessor.is_some()) {
            return Err(EvidenceFreezeError::Contract);
        }
        return Ok(());
    };
    let p = previous.data();
    let mut scope = p.capture.scope.clone();
    scope.request = n.capture.scope.request.clone();
    scope.request_sha256 = n.capture.scope.request_sha256.clone();
    let rights = |d: &NativeDailyEvidence| {
        d.references
            .iter()
            .find(|r| r.kind == EvidenceReferenceKind::Rights)
            .map(|r| r.version.id.clone())
    };
    if scope != n.capture.scope
        || p.capture.scope.request == n.capture.scope.request
        || p.capture.semantics != n.capture.semantics
        || p.capture.range != n.capture.range
        || rights(p) != rights(n)
        || p.captured_at > n.captured_at
    {
        return Err(EvidenceFreezeError::Contract);
    }
    for ((old, row), (new, current)) in p
        .lineage
        .iter()
        .zip(&p.capture.rows)
        .zip(n.lineage.iter().zip(&n.capture.rows))
    {
        if old.observation.id != new.observation.id
            || row.time.source_date != current.time.source_date
        {
            return Err(EvidenceFreezeError::Contract);
        }
        if old.observation == new.observation {
            if row != current || old.predecessor != new.predecessor {
                return Err(EvidenceFreezeError::Contract);
            }
        } else if new.predecessor.as_ref() != Some(&old.observation.revision)
            || new
                .observation
                .revision
                .as_str()
                .parse::<u64>()
                .map_err(contract)?
                <= old
                    .observation
                    .revision
                    .as_str()
                    .parse::<u64>()
                    .map_err(contract)?
            || row.time.received_at > current.time.received_at
        {
            return Err(EvidenceFreezeError::Contract);
        }
    }
    if p.lineage.len() != n.lineage.len() {
        return Err(EvidenceFreezeError::Contract);
    }
    Ok(())
}
