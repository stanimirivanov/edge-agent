//! Native-date capture objects: not whole-series quality acceptance or a publication grant.
use super::*;
use serde::{Deserialize, Serialize};

/// Independent native-date capture format; cannot be passed to daily-v1 storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DateOnlyDailyFormat {
    /// Unadjusted USD/share provider-day capture with explicit temporal witnesses.
    #[serde(rename = "edgeagent.evidence.date-only-daily-capture.v1")]
    V1,
}
/// One immutable captured row; the full retained bucket/calendar records remain required.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DateOnlyDailyRow {
    /// Native label, explicit bucket, actual receipt and derivation provenance.
    pub time: DateOnlyTimeData,
    /// Exact unadjusted USD/share open/high/low/close.
    pub ohlc: [EvidenceDecimal; 4],
    /// Exact nonnegative reported shares, not inferred consolidated volume.
    pub volume: EvidenceDecimal,
}
/// Untrusted capture DTO, deliberately distinct from quality-approved `DailyEvidence`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DateOnlyDailyCapture {
    /// Independent object discriminator.
    pub version: DateOnlyDailyFormat,
    /// Exact authenticated admission scope and request digest.
    pub scope: EvidenceScope,
    /// Reviewed feed semantics; does not grant entitlement.
    pub semantics: EvidenceVersion,
    /// Exact selected routing configuration fingerprint.
    pub selection_sha256: EvidenceDigest,
    /// Exact response body checksum; raw retention still requires separate rights.
    pub response_sha256: EvidenceDigest,
    /// Half-open requested native date range.
    pub range: [EvidenceDate; 2],
    /// Original captured rows, in strictly ascending native-date order.
    #[cfg_attr(feature = "schema", schemars(length(min = 1, max = 252)))]
    pub rows: Vec<DateOnlyDailyRow>,
}
/// Bounded canonical native-date capture object. No implicit conversion to daily-v1.
#[derive(Clone, Eq, PartialEq)]
pub struct DateOnlyDailyArtifact {
    data: DateOnlyDailyCapture,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl DateOnlyDailyArtifact {
    /// Validate exact arithmetic, chronology and bounded ordering, then seal sorted JSON+LF.
    /// Current rights, complete reference authenticity and whole-series quality stay external.
    /// # Errors
    /// Rejects invalid rows, provenance, range, ordering or the 4 MiB object limit.
    pub fn new(data: DateOnlyDailyCapture) -> Result<Self, EvidenceContractError> {
        if data.rows.is_empty() || data.rows.len() > 252 || data.range[0] >= data.range[1] {
            return Err(EvidenceContractError);
        }
        let mut previous = None;
        for row in &data.rows {
            DateOnlyTimeEvidence::new(row.time.clone())?;
            let date = &row.time.source_date;
            if date < &data.range[0]
                || date >= &data.range[1]
                || previous.is_some_and(|p| p >= date)
            {
                return Err(EvidenceContractError);
            }
            previous = Some(date);
            let [open, high, low, close] = [
                row.ohlc[0].scaled(8, true)?,
                row.ohlc[1].scaled(8, true)?,
                row.ohlc[2].scaled(8, true)?,
                row.ohlc[3].scaled(8, true)?,
            ];
            row.volume.scaled(6, false)?;
            if low > high || open < low || open > high || close < low || close > high {
                return Err(EvidenceContractError);
            }
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_EVIDENCE_BYTES {
            return Err(EvidenceContractError);
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before parsing; reject unsupported, tampered and noncanonical objects.
    /// # Errors
    /// Rejects digest, format, shape or invariant failures without payload diagnostics.
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
    /// Checked immutable capture metadata, not quality acceptance.
    #[must_use]
    pub const fn data(&self) -> &DateOnlyDailyCapture {
        &self.data
    }
    /// Exact canonical retained bytes, subject to current rights.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content identity, not a bearer capability.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
/// Consumer dispatch explicitly distinguishes accepted precise evidence from native captures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DailyObject {
    /// Existing quality-approved daily-v1 object.
    Precise(DailyEvidenceArtifact),
    /// Native-date capture still requiring native-date quality/runtime activation.
    DateOnly(DateOnlyDailyArtifact),
}
impl DailyObject {
    /// Dispatch only supported formats, then use their strict canonical decoder.
    /// # Errors
    /// Rejects unknown formats, capacity, integrity or object validation failures.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_EVIDENCE_BYTES || &DailyEvidenceArtifact::hash(bytes) != expected {
            return Err(EvidenceContractError);
        }
        #[derive(Deserialize)]
        struct Header<'a> {
            version: &'a str,
        }
        let header: Header<'_> =
            serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?;
        match header.version {
            "edgeagent.evidence.date-only-daily-capture.v1" => {
                DateOnlyDailyArtifact::decode(bytes, expected).map(Self::DateOnly)
            }
            "edgeagent.evidence.daily.v1" => {
                DailyEvidenceArtifact::decode(bytes, expected).map(Self::Precise)
            }
            _ => Err(EvidenceContractError),
        }
    }
}
redacted!(
    DateOnlyDailyRow,
    DateOnlyDailyCapture,
    DateOnlyDailyArtifact
);
