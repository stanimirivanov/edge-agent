//! Producer-side bridge from checked authority/quality to transport-owned evidence DTOs.
use crate::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::*;
use std::collections::BTreeMap;

/// Explicit frozen inputs, not permission to publish or access stored predecessors.
#[derive(Debug)]
pub struct EvidenceFreeze<'a> {
    /// Owner-authenticated immutable request reference; owner verifies its record digest.
    pub command: &'a AcquireEvidence,
    /// Untrusted provider batch, revalidated before use.
    pub batch: &'a ProviderBatch,
    /// Exact requested range, bucket schedule, identity and cutoff.
    pub series: &'a DailySeriesSpec<'a>,
    /// Complete retained reference catalogs.
    pub references: DailyReferences<'a>,
    /// Explicit quality policy, including thresholds with no defaults.
    pub policy: DailyQualityPolicy,
    /// Prior immutable snapshot for a correction; caller authenticates/authorizes its read.
    pub previous: Option<&'a DailyEvidenceArtifact>,
}
/// Payload-free freezing failure, preserving policy category without source text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EvidenceFreezeError {
    /// Current authority, request or response failed admission/conformance.
    Acquisition(AcquisitionError),
    /// Whole-series domain quality rejected the input.
    Quality(DailyQualityError),
    /// Wire encoding, binding or correction lineage is inconsistent.
    Contract,
}
impl std::fmt::Display for EvidenceFreezeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "evidence freeze rejected: {self:?}")
    }
}
impl std::error::Error for EvidenceFreezeError {}
impl From<EvidenceContractError> for EvidenceFreezeError {
    fn from(_: EvidenceContractError) -> Self {
        Self::Contract
    }
}
pub(super) fn id(value: u128) -> Result<EvidenceId, EvidenceContractError> {
    EvidenceId::new(&format!("{value:032x}"))
}
pub(super) fn revision(value: u64) -> Result<EvidenceRevision, EvidenceContractError> {
    EvidenceRevision::new(&value.to_string())
}
pub(super) fn version(id_value: u128, rev: u64) -> Result<EvidenceVersion, EvidenceContractError> {
    Ok(EvidenceVersion {
        id: id(id_value)?,
        revision: revision(rev)?,
    })
}
pub(super) fn time(value: PreciseInstant) -> Result<EvidenceTime, EvidenceContractError> {
    let (seconds, nanos) = value.unix_parts();
    Ok(EvidenceTime {
        seconds,
        nanos: u32::try_from(nanos).map_err(|_| EvidenceContractError)?,
    })
}
pub(super) fn millis(value: UtcInstant) -> Result<EvidenceTime, EvidenceContractError> {
    time(PreciseInstant::from_millis(value))
}
pub(super) fn date(value: SessionDate) -> Result<EvidenceDate, EvidenceContractError> {
    let (y, m, d) = value.ymd();
    EvidenceDate::new(&format!("{y:04}-{m:02}-{d:02}"))
}
fn decimal(value: (i128, u32)) -> Result<EvidenceDecimal, EvidenceContractError> {
    Ok(EvidenceDecimal {
        coefficient: value.0.to_string(),
        scale: u8::try_from(value.1).map_err(|_| EvidenceContractError)?,
    })
}
pub(super) fn witness<T>(
    kind: EvidenceReferenceKind,
    v: &ReferenceVersion<T>,
    at: Option<SessionDate>,
) -> Result<EvidenceReference, EvidenceContractError> {
    let checksum = v
        .checksum()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok(EvidenceReference {
        kind,
        version: version(v.id().get(), v.revision().get())?,
        source: id(v.source().get())?,
        known_at: millis(v.known_at())?,
        source_sha256: EvidenceDigest::new(&checksum)?,
        date: at.map(date).transpose()?,
    })
}
/// Recheck authority and complete daily quality, then seal deterministic evidence bytes.
/// Only one qualified source is represented; comparative-source evidence needs its own licensed profile.
/// This function performs no provider call, storage write, publication or wall-clock read.
/// # Errors
/// Rejects mismatched inputs, bad authority/quality, invalid wire data or cross-scope correction reuse.
pub fn freeze_evidence(
    admitted: &AuthorizedAcquisition<'_>,
    catalogs: AuthorityCatalogs<'_>,
    now: UtcInstant,
    cancellation: &Cancellation,
    input: EvidenceFreeze<'_>,
) -> Result<DailyEvidenceArtifact, EvidenceFreezeError> {
    validate_batch(admitted, catalogs, now, cancellation, input.batch)
        .map_err(EvidenceFreezeError::Acquisition)?;
    let r = admitted.request();
    let spec = input.series;
    let cutoff = match r.knowledge {
        KnowledgeMode::Capture => now,
        KnowledgeMode::RetainedAt(at) => at,
    };
    if input.command.request != id(r.id.get())?
        || r.range != spec.range
        || r.instrument() != spec.instrument
        || spec.cutoff != cutoff
        || spec.provider != input.batch.provider
    {
        return Err(EvidenceFreezeError::Contract);
    }
    let checked = input
        .policy
        .check(spec, input.references, &input.batch.observations, None)
        .map_err(EvidenceFreezeError::Quality)?;
    let s = r.scope;
    let scope = EvidenceScope {
        request: id(r.id.get())?,
        request_sha256: input.command.request_sha256.clone(),
        principal: id(s.principal.get())?,
        account: id(s.account.get())?,
        source: id(s.source.get())?,
        instrument: id(s.instrument.get())?,
        listing: id(spec.listing.get())?,
        audience: id(s.audience.get())?,
        storage: id(s.storage.get())?,
        endpoint: id(s.endpoint.get())?,
    };
    let semantics = input.batch.provider.declaration().0.rules().0.version();
    let semantic_version = version(semantics.0.get(), semantics.1.get())?;
    let (rights, provider) = admitted.authority();
    if let Some(previous) = input.previous {
        let p = previous.data();
        let mut prior_scope = p.scope.clone();
        prior_scope.request = scope.request.clone();
        prior_scope.request_sha256 = scope.request_sha256.clone();
        let prior_rights = p
            .references
            .iter()
            .find(|v| v.kind == EvidenceReferenceKind::Rights)
            .ok_or(EvidenceFreezeError::Contract)?;
        if p.scope.request == scope.request
            || prior_scope != scope
            || p.semantics != semantic_version
            || p.range != [date(r.range.bounds().0)?, date(r.range.bounds().1)?]
            || prior_rights.version.id != id(rights.id().get())?
            || p.captured_at > millis(now)?
            || p.cutoff > millis(cutoff)?
        {
            return Err(EvidenceFreezeError::Contract);
        }
    }
    let mut references = BTreeMap::new();
    let mut add = |v: EvidenceReference| {
        references.insert((v.kind, v.version.clone()), v);
    };
    add(witness(EvidenceReferenceKind::Rights, rights, None)?);
    add(witness(
        EvidenceReferenceKind::ProviderReview,
        provider,
        None,
    )?);
    add(witness(
        EvidenceReferenceKind::ActionCoverage,
        checked.provenance().2,
        None,
    )?);
    for calendar in checked.calendars() {
        add(witness(
            EvidenceReferenceKind::Calendar,
            calendar,
            Some(calendar.value().key().1),
        )?);
    }
    let mut bars = Vec::new();
    for day in checked.days() {
        let o = day.observation;
        let Some(ObservationValue::Bar(raw)) = o.value() else {
            return Err(EvidenceFreezeError::Contract);
        };
        let label = o.context().session_conditions().0;
        let conditions = o.context().session_conditions().1.to_vec();
        let (observation_id, observation_revision, predecessor) = o.version().lineage();
        let (observed, published) = o.times().provider_times();
        let mapping = witness(EvidenceReferenceKind::Mapping, day.mapping, None)?;
        let calendar = witness(EvidenceReferenceKind::Calendar, day.calendar, Some(label))?;
        let mut actions = Vec::new();
        for action in &day.actions {
            let w = witness(EvidenceReferenceKind::Action, action, Some(label))?;
            actions.push(w.version.clone());
            add(w);
        }
        actions.sort();
        let [open, high, low, close] = raw.values().0;
        bars.push(EvidenceBar {
            date: date(label)?,
            observation: version(observation_id.get(), observation_revision.get())?,
            predecessor: predecessor.map(|r| revision(r.get())).transpose()?,
            transformation: revision(o.context().origin().2.get())?,
            sequence: o
                .sequence()
                .map(|s| EvidenceSequence::new(&s.to_string()))
                .transpose()?,
            bucket: [
                time(raw.bucket().0.bounds().0)?,
                time(raw.bucket().0.bounds().1)?,
            ],
            ohlc: [
                decimal(open.decimal_parts())?,
                decimal(high.decimal_parts())?,
                decimal(low.decimal_parts())?,
                decimal(close.decimal_parts())?,
            ],
            volume: decimal(raw.values().1.decimal_parts())?,
            event_at: time(o.times().event_at())?,
            received_at: time(o.times().received_at())?,
            observed_at: observed.map(time).transpose()?,
            published_at: published.map(time).transpose()?,
            conditions,
            mapping: mapping.version.clone(),
            calendar: calendar.version.clone(),
            actions,
        });
        add(mapping);
        add(calendar);
    }
    let (rev, age, close_bps, source_bps) = input.policy.parameters();
    Ok(DailyEvidenceArtifact::new(DailyEvidence {
        version: EvidenceFormat::V1,
        scope,
        semantics: semantic_version,
        range: [date(r.range.bounds().0)?, date(r.range.bounds().1)?],
        knowledge: match r.knowledge {
            KnowledgeMode::Capture => EvidenceKnowledge::Capture,
            KnowledgeMode::RetainedAt(_) => EvidenceKnowledge::Retained,
        },
        cutoff: millis(cutoff)?,
        captured_at: millis(now)?,
        delete_by: millis(admitted.delete_by())?,
        quality: EvidenceQuality {
            revision: revision(rev.get())?,
            reference_age_ms: EvidenceAgeMillis::new(&age.to_string())?,
            close_change_bps: close_bps,
            source_difference_bps: source_bps,
        },
        references: references.into_values().collect(),
        bars,
        replaces: input.previous.map(|p| p.digest().clone()),
    })?)
}
