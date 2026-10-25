//! Accepted native snapshots, distinct from raw captures and precise daily-v1.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Independent quality-accepted native format. Old readers must reject it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum NativeDailyFormat {
    /// Raw USD/share provider-day snapshot with native dates and explicit quality provenance.
    #[serde(rename = "edgeagent.evidence.native-daily.v1")]
    V1,
}
/// Owner-assigned lineage and selected references for the corresponding captured row.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NativeDailyLineage {
    /// Service-owned source observation identity/revision, not a vendor assertion.
    pub observation: EvidenceVersion,
    /// Previous retained revision, explicitly null for original revision one.
    #[serde(deserialize_with = "super::model::required")]
    pub predecessor: Option<EvidenceRevision>,
    /// Exact deterministic row transformation revision.
    pub transformation: EvidenceRevision,
    /// Selected point-in-time mapping version.
    pub mapping: EvidenceVersion,
    /// Selected supported action versions, retained as annotations only.
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
    pub actions: Vec<EvidenceVersion>,
}
/// Untrusted quality-result DTO. Decoding is not proof of authority or producer execution.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NativeDailyEvidence {
    /// Separate accepted-object discriminator.
    pub version: NativeDailyFormat,
    /// Original native capture including exact response/selection identities and receipt times.
    pub capture: DateOnlyDailyCapture,
    /// Actual acceptance/capture cutoff, never historical availability inferred from a label.
    pub captured_at: EvidenceTime,
    /// Exclusive original retention deadline, at most 24 hours after acceptance.
    pub delete_by: EvidenceTime,
    /// Exact whole-series quality configuration.
    pub quality: EvidenceQuality,
    /// Complete selected reference witnesses; authenticate and retain full records separately.
    #[cfg_attr(feature = "schema", schemars(length(max = 4717)))]
    pub references: Vec<EvidenceReference>,
    /// One lineage record per captured row, in the same ascending-date order.
    #[cfg_attr(feature = "schema", schemars(length(min = 1, max = 252)))]
    pub lineage: Vec<NativeDailyLineage>,
    /// Exact predecessor object, null for an original snapshot.
    #[serde(deserialize_with = "super::model::required")]
    pub replaces: Option<EvidenceDigest>,
}
/// Canonical accepted-native bytes. No conversion to precise daily evidence or publication grant.
///
/// ```compile_fail,E0277
/// use edgeagent_contracts::evidence::{DateOnlyDailyArtifact, NativeDailyArtifact};
/// fn capture_is_not_acceptance(value: DateOnlyDailyArtifact) -> NativeDailyArtifact {
///     value.into()
/// }
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct NativeDailyArtifact {
    data: NativeDailyEvidence,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl NativeDailyArtifact {
    /// Validate wire invariants and seal sorted JSON+LF. Quality/authority stay producer obligations.
    /// # Errors
    /// Rejects incomplete provenance, chronology, lineage, range or byte bounds.
    pub fn new(mut data: NativeDailyEvidence) -> Result<Self, EvidenceContractError> {
        validate(&data)?;
        data.references
            .sort_by(|a, b| (a.kind, &a.version).cmp(&(b.kind, &b.version)));
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        let digest = DailyEvidenceArtifact::hash(&bytes);
        if bytes.len() > MAX_EVIDENCE_BYTES || data.replaces.as_ref() == Some(&digest) {
            return Err(EvidenceContractError);
        }
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before parsing; reject tampering, unsupported formats and alternate encodings.
    /// # Errors
    /// Returns a payload-free contract failure, never repairs an object.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_EVIDENCE_BYTES || &DailyEvidenceArtifact::hash(bytes) != expected {
            return Err(EvidenceContractError);
        }
        let value = Self::new(serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?)?;
        if value.bytes != bytes {
            return Err(EvidenceContractError);
        }
        Ok(value)
    }
    /// Checked immutable wire data, not trusted policy or read permission.
    #[must_use]
    pub const fn data(&self) -> &NativeDailyEvidence {
        &self.data
    }
    /// Exact canonical bytes, subject to current rights.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Byte identity, never an access capability.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
fn validate(d: &NativeDailyEvidence) -> Result<(), EvidenceContractError> {
    let e = EvidenceContractError;
    // Check collection bounds before cloning/serializing the embedded capture.
    if d.capture.rows.is_empty()
        || d.capture.rows.len() > 252
        || d.references.len() > 4717
        || d.lineage.len() != d.capture.rows.len()
    {
        return Err(e);
    }
    DateOnlyDailyArtifact::new(d.capture.clone())?;
    let (start, end) = (d.capture.range[0].date()?, d.capture.range[1].date()?);
    let n = (end - start).num_days();
    if !(1..=366).contains(&n)
        || d.lineage.len() != d.capture.rows.len()
        || d.references.len() > 4717
        || !d.captured_at.valid()
        || !d.delete_by.valid()
        || !d.captured_at.nanos.is_multiple_of(1_000_000)
        || !d.delete_by.nanos.is_multiple_of(1_000_000)
        || d.delete_by <= d.captured_at
        || d.delete_by.millis() - d.captured_at.millis() > 86_400_000
    {
        return Err(e);
    }
    let age = d
        .quality
        .reference_age_ms
        .as_str()
        .parse::<i128>()
        .map_err(|_| e)?;
    let fresh = |t: EvidenceTime| {
        t.valid() && t <= d.captured_at && d.captured_at.millis() - t.millis() <= age
    };
    let mut refs = BTreeMap::new();
    let mut calendars = BTreeMap::new();
    for r in &d.references {
        if !fresh(r.known_at)
            || refs.insert((r.kind, &r.version), r).is_some()
            || matches!(
                r.kind,
                EvidenceReferenceKind::Calendar | EvidenceReferenceKind::Action
            ) != r.date.is_some()
        {
            return Err(e);
        }
        if r.kind == EvidenceReferenceKind::Calendar {
            let date = r.date.as_ref().ok_or(e)?;
            if date < &d.capture.range[0]
                || date >= &d.capture.range[1]
                || calendars.insert(date, r).is_some()
            {
                return Err(e);
            }
        }
    }
    if calendars.len() as i64 != n {
        return Err(e);
    }
    for kind in [
        EvidenceReferenceKind::Rights,
        EvidenceReferenceKind::ProviderReview,
        EvidenceReferenceKind::ActionCoverage,
    ] {
        if refs.keys().filter(|(k, _)| *k == kind).count() != 1 {
            return Err(e);
        }
    }
    let mut ids = BTreeSet::new();
    let mut used = BTreeSet::new();
    let mut previous_end = None;
    for (row, line) in d.capture.rows.iter().zip(&d.lineage) {
        let t = &row.time;
        if t.derived_at > d.captured_at
            || !fresh(t.rule.known_at)
            || !fresh(t.calendar.known_at)
            || previous_end.is_some_and(|end| t.bucket[0] < end)
            || d.captured_at.millis() - t.bucket[1].millis() < 43_200_000
            || !ids.insert(&line.observation.id)
            || line.actions.len() > 16
            || line.actions.windows(2).any(|w| matches!(w,[a,b] if a>=b))
        {
            return Err(e);
        }
        previous_end = Some(t.bucket[1]);
        let revision = line
            .observation
            .revision
            .as_str()
            .parse::<u64>()
            .map_err(|_| e)?;
        if (d.replaces.is_none() && line.predecessor.is_some())
            || match &line.predecessor {
                None => revision != 1,
                Some(p) => p.as_str().parse::<u64>().map_err(|_| e)? >= revision,
            }
        {
            return Err(e);
        }
        if calendars.get(&t.source_date).copied() != Some(&t.calendar)
            || !refs.contains_key(&(EvidenceReferenceKind::Mapping, &line.mapping))
        {
            return Err(e);
        }
        used.insert((EvidenceReferenceKind::Mapping, &line.mapping));
        for action in &line.actions {
            let r = refs
                .get(&(EvidenceReferenceKind::Action, action))
                .ok_or(e)?;
            if r.date.as_ref() != Some(&t.source_date) {
                return Err(e);
            }
            used.insert((EvidenceReferenceKind::Action, action));
        }
    }
    if refs.keys().any(|(k, v)| {
        matches!(
            k,
            EvidenceReferenceKind::Mapping | EvidenceReferenceKind::Action
        ) && !used.contains(&(*k, *v))
    }) {
        return Err(e);
    }
    Ok(())
}
redacted!(NativeDailyLineage, NativeDailyEvidence, NativeDailyArtifact);
