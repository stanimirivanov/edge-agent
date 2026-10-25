//! Owner-private configuration, not a signed grant or a credential container.
use super::*;
use serde::{Deserialize, Serialize};

/// Exact initial provider protocols; neither selection implies account eligibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationProvider {
    /// Native-date, unadjusted consolidated daily snapshots.
    TwelveDataDaily,
    /// Precise source opening instants, historical SIP and raw adjustments.
    AlpacaHistoricalSipDaily,
}
/// Independently versioned owner-private configuration encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum QualificationProfileFormat {
    /// First finite daily qualification profile; unknown versions fail closed.
    #[serde(rename = "edgeagent.qualification-profile.v1")]
    V1,
}
/// Untrusted configuration. Authenticity/approval must come from the separate owner port.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationProfileData {
    /// Exact encoding version.
    pub version: QualificationProfileFormat,
    /// One reviewed provider protocol, without endpoint overrides or fallback.
    pub provider: QualificationProvider,
    /// Complete immutable acquisition identity.
    pub admission: AcquireEvidence,
    /// Frozen schedule/selection/recipe digest already bound into admission.
    pub configuration_sha256: EvidenceDigest,
    /// Expected independently authenticated owner opt-in review revision.
    pub owner_review: EvidenceVersion,
    /// Exact installed exclusive allocation and reviewed maximum endpoint weights.
    pub credits: DailyCreditPolicy,
    /// Inclusive finite run-window start, not evidence availability.
    pub not_before: EvidenceTime,
    /// Exclusive run-window end; never extends the admitted command deadline.
    pub expires_at: EvidenceTime,
}
impl fmt::Debug for QualificationProfileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QualificationProfileData(..)")
    }
}
/// Bounded canonical configuration. Decode/new grants no authority and performs no I/O.
#[derive(Clone, Eq, PartialEq)]
pub struct QualificationProfile {
    data: QualificationProfileData,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl fmt::Debug for QualificationProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QualificationProfile(..)")
    }
}
impl QualificationProfile {
    /// Seal recursively sorted JSON plus one LF, at most 8 KiB.
    /// # Errors
    /// Rejects malformed milliseconds, nonpositive/over-300-second windows or oversized input.
    pub fn new(data: QualificationProfileData) -> Result<Self, BudgetError> {
        let start = instant(data.not_before)?;
        let end = instant(data.expires_at)?;
        if end <= start || end.unix_millis() - start.unix_millis() > 300_000 {
            return Err(error(K::Invalid));
        }
        let value =
            serde_json::to_value(&data).map_err(|e| BudgetError::with_source(K::Invalid, e))?;
        let mut bytes = crate::canonical::canonical(value)?;
        bytes.push(b'\n');
        if bytes.len() > 8192 {
            return Err(error(K::Invalid));
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before decoding; require the expected digest and exact canonical representation.
    /// # Errors
    /// Rejects unknown/duplicate/missing fields, alternate encodings and changed identities.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, BudgetError> {
        if bytes.len() > 8192 || &DailyEvidenceArtifact::hash(bytes) != expected {
            return Err(error(K::Invalid));
        }
        let data =
            serde_json::from_slice(bytes).map_err(|e| BudgetError::with_source(K::Invalid, e))?;
        let profile = Self::new(data)?;
        if profile.bytes != bytes {
            return Err(error(K::Invalid));
        }
        Ok(profile)
    }
    /// Exact configuration values; these are declarations, not authenticated reviews.
    #[must_use]
    pub const fn data(&self) -> &QualificationProfileData {
        &self.data
    }
    /// Canonical owner-private bytes; do not log or publish them.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content pin, never a bearer capability or substitute for owner authentication.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
