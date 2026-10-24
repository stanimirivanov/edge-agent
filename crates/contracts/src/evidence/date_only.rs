//! Versioned temporal value, not a daily-v1 artifact or a publication/authority grant.
use super::*;
use serde::{Deserialize, Serialize};

/// Capacity for one canonical temporal value, including its final LF.
pub const MAX_DATE_ONLY_TIME_BYTES: usize = 4096;
/// Native-date temporal format; distinct from existing daily-v1 object semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DateOnlyTimeFormat {
    /// Exact native label and explicit reviewed UTC bucket derivation.
    #[serde(rename = "edgeagent.evidence.date-only-time.v1")]
    V1,
}
/// Supported date-label zone, not a request to perform timezone conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DateOnlyTimezone {
    /// The current US equity profile; other zones require explicit qualification.
    #[serde(rename = "America/New_York")]
    AmericaNewYork,
}
/// Exact rule witness. Its full reviewed source record must be retained by the owner.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DateOnlyRuleWitness {
    /// Exact bucket-rule identity/revision, not the feed's general semantic revision.
    pub version: EvidenceVersion,
    /// Rule source authority identity.
    pub source: EvidenceId,
    /// Actual local receipt of the rule.
    pub known_at: EvidenceTime,
    /// SHA-256 of retained source bytes, not a bearer grant or proof of authenticity.
    pub source_sha256: EvidenceDigest,
    /// Exact calendar dependency selected by that rule.
    pub calendar: EvidenceVersion,
}
/// Untrusted date-only temporal DTO. Only `DateOnlyTimeEvidence` seals invariants.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DateOnlyTimeData {
    /// Independent immutable value format.
    pub version: DateOnlyTimeFormat,
    /// Provider's native Gregorian label, never expanded into a precise source event.
    pub source_date: EvidenceDate,
    /// Explicit half-open derived UTC bucket; never core-session/midnight defaults.
    pub bucket: [EvidenceTime; 2],
    /// Actual local receipt, not historical knowledge inferred from the label.
    pub received_at: EvidenceTime,
    /// Actual local derivation time, at or after receipt and both reference witnesses.
    pub derived_at: EvidenceTime,
    /// Required reviewed bucket rule and retained source provenance.
    pub rule: DateOnlyRuleWitness,
    /// Required exact dated calendar witness, not proof by a calendar ID alone.
    pub calendar: EvidenceReference,
    /// Label timezone under the qualified profile.
    pub timezone: DateOnlyTimezone,
    /// Bounded named timezone-data revision, not the library's current default.
    #[cfg_attr(
        feature = "schema",
        schemars(length(min = 1, max = 32), regex(pattern = "^[A-Za-z0-9._-]+$"))
    )]
    pub tzdata_revision: String,
}
/// Sealed, canonical temporal provenance. It is intentionally not convertible to
/// `DailyEvidenceArtifact`; future observation/object versions must embed it explicitly.
#[derive(Clone, Eq, PartialEq)]
pub struct DateOnlyTimeEvidence {
    data: DateOnlyTimeData,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl DateOnlyTimeEvidence {
    /// Validate chronology/provenance and seal recursively sorted JSON plus one LF.
    /// Authenticity, exchange correctness and current rights remain owner obligations.
    /// # Errors
    /// Rejects mismatched calendar pins/dates, invalid bounds, chronology or capacity.
    pub fn new(data: DateOnlyTimeData) -> Result<Self, EvidenceContractError> {
        let [start, end] = data.bucket;
        let r = &data.rule;
        let c = &data.calendar;
        if [
            start,
            end,
            data.received_at,
            data.derived_at,
            r.known_at,
            c.known_at,
        ]
        .iter()
        .any(|t| !t.valid())
            || start >= end
            || start.nanos % 1_000_000 != 0
            || end.nanos % 1_000_000 != 0
            || end.millis() - start.millis() > 93_600_000
            || data.received_at < end
            || data.derived_at < data.received_at
            || r.known_at > data.derived_at
            || c.known_at > data.derived_at
            || c.kind != EvidenceReferenceKind::Calendar
            || c.date.as_ref() != Some(&data.source_date)
            || r.calendar != c.version
            || data.tzdata_revision.is_empty()
            || data.tzdata_revision.len() > 32
            || !data
                .tzdata_revision
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err(EvidenceContractError);
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_DATE_ONLY_TIME_BYTES {
            return Err(EvidenceContractError);
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before parsing, check digest and reject every alternate byte representation.
    /// # Errors
    /// Rejects oversized, unsupported, unknown/duplicate/noncanonical or tampered data.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_DATE_ONLY_TIME_BYTES || &DailyEvidenceArtifact::hash(bytes) != expected
        {
            return Err(EvidenceContractError);
        }
        let data = serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?;
        let value = Self::new(data)?;
        if value.bytes != bytes {
            return Err(EvidenceContractError);
        }
        Ok(value)
    }
    /// Immutable checked data; this grants no current evidence access or entitlement.
    #[must_use]
    pub const fn data(&self) -> &DateOnlyTimeData {
        &self.data
    }
    /// Exact canonical JSON bytes including final LF.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content identity, never evidence of authenticity or historical availability.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
redacted!(DateOnlyTimeEvidence, DateOnlyTimeData, DateOnlyRuleWitness);
impl Serialize for DateOnlyTimeEvidence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.data.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for DateOnlyTimeEvidence {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::new(DateOnlyTimeData::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}
