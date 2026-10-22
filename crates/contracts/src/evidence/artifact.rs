use super::{model::EvidenceReferenceKind as Kind, *};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Sealed canonical daily-v1 object. Retain exact bytes, not a reserialized wire value.
#[derive(Clone, Eq, PartialEq)]
pub struct DailyEvidenceArtifact {
    data: DailyEvidence,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl DailyEvidenceArtifact {
    /// Validate bounded wire/lineage invariants and sort the reference census.
    /// Domain policy and authenticated input authority remain producer obligations.
    /// # Errors
    /// Rejects invalid provenance, units, chronology, coverage or object capacity.
    pub fn new(mut data: DailyEvidence) -> Result<Self, EvidenceContractError> {
        validate(&data)?;
        data.references
            .sort_by(|a, b| (a.kind, &a.version).cmp(&(b.kind, &b.version)));
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_EVIDENCE_BYTES {
            return Err(EvidenceContractError);
        }
        let digest = Self::hash(&bytes);
        if data.replaces.as_ref() == Some(&digest) {
            return Err(EvidenceContractError);
        }
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before parsing, verify SHA-256, and require exact canonical v1 bytes.
    /// Whitespace, duplicate/unknown keys and alternate encodings are not repaired.
    /// # Errors
    /// Rejects oversized, corrupt, unsupported or noncanonical objects without payload diagnostics.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_EVIDENCE_BYTES || &Self::hash(bytes) != expected {
            return Err(EvidenceContractError);
        }
        let data: DailyEvidence =
            serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?;
        let artifact = Self::new(data)?;
        if artifact.bytes != bytes {
            return Err(EvidenceContractError);
        }
        Ok(artifact)
    }
    /// SHA-256 over exact bytes; hashing alone never validates their content or license.
    #[must_use]
    pub fn hash(bytes: &[u8]) -> EvidenceDigest {
        let mut text = String::with_capacity(64);
        let digit = |n: u8| char::from(if n < 10 { b'0' + n } else { b'a' + n - 10 });
        for byte in Sha256::digest(bytes) {
            text.push(digit(byte >> 4));
            text.push(digit(byte & 15));
        }
        EvidenceDigest(text)
    }
    /// Canonical compact JSON, sorted keys and one final LF; not RFC 8785 for arbitrary JSON.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content identity bound to complete scope, provenance and rows.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
    /// Immutable validated wire data; referenced source records still require owner retention.
    #[must_use]
    pub const fn data(&self) -> &DailyEvidence {
        &self.data
    }
}
impl Serialize for DailyEvidenceArtifact {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.data.serialize(s)
    }
}
impl<'de> Deserialize<'de> for DailyEvidenceArtifact {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(DailyEvidence::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
redacted!(DailyEvidenceArtifact);

fn validate(d: &DailyEvidence) -> Result<(), EvidenceContractError> {
    let error = EvidenceContractError;
    let (start, end) = (d.range[0].date()?, d.range[1].date()?);
    let days = (end - start).num_days();
    if !(1..=366).contains(&days)
        || !(1..=252).contains(&d.bars.len())
        || d.references.len() > 4717
        || [d.cutoff, d.captured_at, d.delete_by]
            .iter()
            .any(|t| !t.valid())
        || d.cutoff > d.captured_at
        || (d.knowledge == EvidenceKnowledge::Capture && d.cutoff != d.captured_at)
        || d.delete_by <= d.captured_at
        || d.delete_by.millis() - d.captured_at.millis() > 86_400_000
    {
        return Err(error);
    }
    if [d.cutoff, d.captured_at, d.delete_by]
        .iter()
        .any(|t| t.nanos % 1_000_000 != 0)
    {
        return Err(error);
    }
    let mut refs = BTreeMap::new();
    let mut calendar = BTreeMap::new();
    for r in &d.references {
        let cutoff = if matches!(r.kind, Kind::Rights | Kind::ProviderReview) {
            d.captured_at
        } else {
            d.cutoff
        };
        if !r.known_at.valid()
            || r.known_at.nanos % 1_000_000 != 0
            || r.known_at > cutoff
            || refs.insert((r.kind, r.version.clone()), r).is_some()
        {
            return Err(error);
        }
        if matches!(r.kind, Kind::Calendar | Kind::Action) != r.date.is_some() {
            return Err(error);
        }
        if r.kind == Kind::Calendar
            && calendar
                .insert(r.date.as_ref().ok_or(error)?.clone(), r)
                .is_some()
        {
            return Err(error);
        }
        if !matches!(r.kind, Kind::Rights | Kind::ProviderReview)
            && d.cutoff.millis() - r.known_at.millis()
                > i128::from(
                    d.quality
                        .reference_age_ms
                        .as_str()
                        .parse::<i64>()
                        .map_err(|_| error)?,
                )
        {
            return Err(error);
        }
    }
    if calendar.len() != usize::try_from(days).map_err(|_| error)? {
        return Err(error);
    }
    let mut date = start;
    while date < end {
        let key = EvidenceDate::new(&date.format("%Y-%m-%d").to_string())?;
        if !calendar.contains_key(&key) {
            return Err(error);
        }
        date = date.succ_opt().ok_or(error)?;
    }
    let mut used = BTreeSet::new();
    for (key, _) in refs.iter().filter(|((kind, _), _)| {
        matches!(
            kind,
            Kind::Calendar | Kind::Rights | Kind::ProviderReview | Kind::ActionCoverage
        )
    }) {
        used.insert(key.clone());
    }
    for kind in [Kind::Rights, Kind::ProviderReview, Kind::ActionCoverage] {
        if refs.keys().filter(|(k, _)| *k == kind).count() != 1 {
            return Err(error);
        }
    }
    let mut previous_date = None;
    let mut previous_end = None;
    let mut observations = BTreeSet::new();
    for b in &d.bars {
        let day = b.date.date()?;
        if day < start
            || day >= end
            || previous_date.is_some_and(|v| v >= day)
            || !observations.insert(b.observation.id.clone())
        {
            return Err(error);
        }
        previous_date = Some(day);
        let revision = b
            .observation
            .revision
            .as_str()
            .parse::<u64>()
            .map_err(|_| error)?;
        match &b.predecessor {
            None if revision != 1 => return Err(error),
            Some(p) if p.as_str().parse::<u64>().map_err(|_| error)? >= revision => {
                return Err(error);
            }
            _ => {}
        }
        let [begin, end] = b.bucket;
        if [begin, end, b.event_at, b.received_at]
            .iter()
            .any(|t| !t.valid())
            || begin >= end
            || begin.nanos % 1_000_000 != 0
            || end.nanos % 1_000_000 != 0
            || previous_end.is_some_and(|p| begin < p)
            || b.event_at < begin
            || b.event_at > end
            || b.received_at < end
            || b.received_at > d.cutoff
            || d.cutoff.millis() - end.millis() < 43_200_000
        {
            return Err(error);
        }
        previous_end = Some(end);
        let mut at = b.event_at;
        for t in [b.observed_at, b.published_at, Some(b.received_at)]
            .into_iter()
            .flatten()
        {
            if !t.valid() || t < at {
                return Err(error);
            }
            at = t;
        }
        let [open, high, low, close] = [
            b.ohlc[0].scaled(8, true)?,
            b.ohlc[1].scaled(8, true)?,
            b.ohlc[2].scaled(8, true)?,
            b.ohlc[3].scaled(8, true)?,
        ];
        if low > high || open < low || open > high || close < low || close > high {
            return Err(error);
        }
        b.volume.scaled(6, false)?;
        if b.actions.len() > 4096
            || b.conditions.len() > 16
            || b.conditions.windows(2).any(|w| matches!(w,[a,b] if a>=b))
        {
            return Err(error);
        }
        for (kind, key) in [(Kind::Mapping, &b.mapping), (Kind::Calendar, &b.calendar)]
            .into_iter()
            .chain(b.actions.iter().map(|r| (Kind::Action, r)))
        {
            let reference = refs.get(&(kind, key.clone())).ok_or(error)?;
            if matches!(kind, Kind::Calendar | Kind::Action)
                && reference.date.as_ref() != Some(&b.date)
            {
                return Err(error);
            }
            if !used.insert((kind, key.clone())) && kind == Kind::Action {
                return Err(error);
            }
        }
    }
    if used.len() != refs.len() {
        return Err(error);
    }
    Ok(())
}
