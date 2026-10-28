//! Native interpretation evidence, distinct from canonical observations and raw frames.
use super::*;
use serde::{Deserialize, Serialize};

/// Versioned native projection; new providers require an explicitly reviewed variant.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StreamNativeValue {
    /// Original IEX trade identity and condition order; not source sequencing.
    IexTrade {
        /// Exact positive provider ID, including values above 2^53.
        id: EvidenceRevision,
        /// Original source exchange code.
        exchange: String,
        /// Original source tape code.
        tape: String,
        /// Original source ordering, never replaced by canonical eligibility codes.
        conditions: Vec<String>,
    },
    /// Original IEX minute aggregate metadata; neither count nor VWAP is reconstructed.
    IexMinuteBar {
        /// Exact reported count, including zero.
        trades: EvidenceSequence,
        /// Exact positive USD/share provider VWAP.
        vwap: EvidenceDecimal,
    },
}
/// One-to-one native metadata at the canonical record's local position.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamNativeRecord {
    /// Complete-message frame range, including interleaved controls.
    pub frames: [EvidenceRevision; 2],
    /// Original zero-based item position before filtering.
    pub item: u32,
    /// Typed original metadata; normalized values/times reside in the bound segment.
    pub value: StreamNativeValue,
}
/// The only supported native companion format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StreamCompanionFormat {
    /// Recursively canonical JSON plus LF, exact SHA-256 including LF.
    #[serde(rename = "edgeagent.evidence.stream-companion.v1")]
    V1,
}
/// Untrusted companion input; seal before storage or publication.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamCompanionData {
    /// Explicit supported format.
    pub version: StreamCompanionFormat,
    /// Explicit operator byte/record ceilings, subject to portable segment ceilings.
    pub limits: StreamSegmentLimits,
    /// Exact canonical object, full scope and the same exclusive deletion deadline.
    pub segment: StreamSegmentObject,
    /// Complete canonical owner-input record, not just a profile hash or raw frames.
    pub owner_inputs: Vec<u8>,
    /// Exact native metadata in canonical record order, including empty control-only capture.
    pub records: Vec<StreamNativeRecord>,
}
/// Sealed native companion. Codec validity grants no rights or independent approval.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamCompanion {
    data: StreamCompanionData,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
/// Owner-private companion location, never a read or cleanup capability.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamCompanionObject {
    /// Canonical object providing exact shared scope, deadline and content binding.
    pub segment: StreamSegmentObject,
    /// Native content identity, distinct from canonical content.
    pub sha256: EvidenceDigest,
    /// Exact bounded encoded size, including LF.
    pub bytes: u32,
}
fn code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 8 && s.bytes().all(|b| b.is_ascii_graphic())
}
impl StreamCompanion {
    /// Seal bounded typed native metadata. Application validates owner bytes and canonical binding.
    /// # Errors
    /// Rejects invalid ceilings, positions, source metadata or excessive encoded size.
    pub fn new(data: StreamCompanionData) -> Result<Self, EvidenceContractError> {
        if data.limits.bytes == 0
            || data.limits.bytes as usize > MAX_STREAM_SEGMENT_BYTES
            || data.limits.records == 0
            || data.limits.records as usize > MAX_STREAM_SEGMENT_RECORDS
            || data.records.len() > data.limits.records as usize
            || data.owner_inputs.is_empty()
            || data.owner_inputs.len() > 1_048_576
        {
            return Err(EvidenceContractError);
        }
        let mut previous = None;
        for r in &data.records {
            let frame: u64 = r.frames[1]
                .as_str()
                .parse()
                .map_err(|_| EvidenceContractError)?;
            let first: u64 = r.frames[0]
                .as_str()
                .parse()
                .map_err(|_| EvidenceContractError)?;
            if first > frame || previous.is_some_and(|p| p >= (frame, r.item)) {
                return Err(EvidenceContractError);
            }
            previous = Some((frame, r.item));
            match &r.value {
                StreamNativeValue::IexTrade {
                    exchange,
                    tape,
                    conditions,
                    ..
                } => {
                    if !code(exchange)
                        || !code(tape)
                        || conditions.len() > 16
                        || conditions.iter().any(|c| !code(c))
                        || conditions
                            .iter()
                            .enumerate()
                            .any(|(i, c)| conditions.iter().take(i).any(|old| old == c))
                    {
                        return Err(EvidenceContractError);
                    }
                }
                StreamNativeValue::IexMinuteBar { vwap, .. } => {
                    let n: u128 = vwap
                        .coefficient
                        .parse()
                        .map_err(|_| EvidenceContractError)?;
                    if n == 0 || vwap.scale > 18 || n.to_string() != vwap.coefficient {
                        return Err(EvidenceContractError);
                    }
                }
            }
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > data.limits.bytes as usize {
            return Err(EvidenceContractError);
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before parsing/hashing and require exact canonical bytes.
    /// # Errors
    /// Rejects corrupt, unsupported or alternate encodings.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_STREAM_SEGMENT_BYTES || &DailyEvidenceArtifact::hash(bytes) != expected
        {
            return Err(EvidenceContractError);
        }
        let value = Self::new(serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?)?;
        if value.bytes != bytes {
            return Err(EvidenceContractError);
        }
        Ok(value)
    }
    /// Complete private provenance fields; never log them.
    #[must_use]
    pub const fn data(&self) -> &StreamCompanionData {
        &self.data
    }
    /// Exact immutable bytes for create/read-back and uncertain recovery.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Exact content-bound scoped descriptor, not authority.
    #[must_use]
    pub fn object(&self) -> StreamCompanionObject {
        StreamCompanionObject {
            segment: self.data.segment.clone(),
            sha256: self.digest.clone(),
            bytes: self.bytes.len() as u32,
        }
    }
    /// Require exact object and one native item per canonical record, preserving order.
    /// # Errors
    /// Rejects scope/deadline/digest, positions, ranges or native/canonical value-kind mismatch.
    pub fn bind(&self, segment: &StreamSegment) -> Result<(), EvidenceContractError> {
        let d = segment.data();
        if self.data.segment != segment.object() || self.data.records.len() != d.records.len() {
            return Err(EvidenceContractError);
        }
        for (native, canonical) in self.data.records.iter().zip(&d.records) {
            let identity = u128::from_str_radix(canonical.observation.id.as_str(), 16)
                .map_err(|_| EvidenceContractError)?;
            let low = identity as u64;
            let identity_matches = match (&native.value, &canonical.value) {
                (StreamNativeValue::IexTrade { id, .. }, _) => {
                    id.as_str()
                        .parse::<u64>()
                        .map_err(|_| EvidenceContractError)?
                        == low
                }
                (StreamNativeValue::IexMinuteBar { .. }, Some(StreamValue::Bar { bucket, .. })) => {
                    bucket[0].seconds >= 0
                        && bucket[0].nanos == 0
                        && bucket[0].seconds % 60 == 0
                        && u64::try_from(bucket[0].seconds / 60)
                            .map_err(|_| EvidenceContractError)?
                            == low
                }
                _ => false,
            };
            if !identity_matches
                || native.frames[1] != canonical.frame
                || native.item != canonical.item
                || native.frames[0]
                    .as_str()
                    .parse::<u64>()
                    .map_err(|_| EvidenceContractError)?
                    < d.frames[0]
                        .as_str()
                        .parse::<u64>()
                        .map_err(|_| EvidenceContractError)?
                || !matches!(
                    (&native.value, &canonical.value),
                    (
                        StreamNativeValue::IexTrade { .. },
                        Some(StreamValue::Trade { .. })
                    ) | (
                        StreamNativeValue::IexMinuteBar { .. },
                        Some(StreamValue::Bar {
                            coverage: StreamBarCoverage::Intraday,
                            ..
                        })
                    )
                )
            {
                return Err(EvidenceContractError);
            }
        }
        Ok(())
    }
}
redacted!(
    StreamNativeValue,
    StreamNativeRecord,
    StreamCompanionData,
    StreamCompanion,
    StreamCompanionObject
);
