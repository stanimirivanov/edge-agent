//! Normalized capture segments, never a readiness certificate or raw vendor archive.
use super::*;
use serde::{Deserialize, Serialize};

/// Portable allocation ceiling, independent from the smaller event-envelope limit.
pub const MAX_STREAM_SEGMENT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum normalized records in one segment; controls may produce no records.
pub const MAX_STREAM_SEGMENT_RECORDS: usize = 4096;

/// Explicit operator bounds; portable ceilings are not default operating limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamSegmentLimits {
    /// Positive encoded byte limit, at most `MAX_STREAM_SEGMENT_BYTES`.
    pub bytes: u32,
    /// Positive normalized record limit, at most `MAX_STREAM_SEGMENT_RECORDS`.
    pub records: u32,
}

/// Versioned streaming-only encoding. Daily readers deliberately reject it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StreamSegmentFormat {
    /// Compact recursively sorted JSON plus one LF.
    #[serde(rename = "edgeagent.evidence.stream-segment.v1")]
    V1,
}
/// Complete per-instrument licensed namespace, not a credential or storage URL.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamSegmentScope {
    /// Finite admitted session identity.
    pub session: EvidenceId,
    /// Digest of the exact session, including full frozen selection and authority.
    pub session_sha256: EvidenceDigest,
    /// Authenticated principal.
    pub principal: EvidenceId,
    /// Data-only provider account.
    pub account: EvidenceId,
    /// Source/feed identity.
    pub source: EvidenceId,
    /// One selected instrument.
    pub instrument: EvidenceId,
    /// Reviewed endpoint.
    pub endpoint: EvidenceId,
    /// Intended audience.
    pub audience: EvidenceId,
    /// Approved storage scope.
    pub storage: EvidenceId,
    /// Exact rights revision; full retained review remains necessary.
    pub rights: EvidenceVersion,
}
/// Capture delivery declaration; neither variant proves source continuity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamDelivery {
    /// Snapshot-only source, not an ordered complete tape.
    Snapshots,
    /// Source sequence required on every normalized record, including corrections.
    Sequenced,
}
/// Exact option side, preserving absent versus zero displayed contracts.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamQuoteSide {
    /// Nonnegative premium points, not underlying USD/share.
    pub points: EvidenceDecimal,
    /// Whole displayed contracts; null is unknown, not zero.
    #[serde(deserialize_with = "super::model::required")]
    pub size: Option<EvidenceSequence>,
    /// Side-specific source time.
    pub at: EvidenceTime,
}
/// Normalized values only; these wire checks cannot authenticate option specifications.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StreamValue {
    /// Raw USD/share OHLCV with an explicit bucket and coverage.
    Bar {
        /// Explicit intraday/core coverage, never provider-day.
        coverage: StreamBarCoverage,
        /// Inclusive start, exclusive end.
        bucket: [EvidenceTime; 2],
        /// Open, high, low, close.
        ohlc: [EvidenceDecimal; 4],
        /// Nonnegative shares.
        volume: EvidenceDecimal,
    },
    /// Positive USD/share and positive shares.
    Trade {
        /// Exact trade price.
        price: EvidenceDecimal,
        /// Exact positive share quantity.
        shares: EvidenceDecimal,
    },
    /// Positive, non-tradable cash-index points.
    Index {
        /// Exact index level.
        points: EvidenceDecimal,
    },
    /// Two-sided option premium snapshot.
    OptionQuote {
        /// Exact externally authenticated specification revision.
        specification: EvidenceVersion,
        /// Bid side.
        bid: StreamQuoteSide,
        /// Ask side.
        ask: StreamQuoteSide,
    },
}
/// Coverage is explicit and cannot include the incompatible daily format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamBarCoverage {
    /// Qualified core-session bucket.
    Core,
    /// Other explicit intraday bucket.
    Intraday,
}
/// One normalized source revision at a local frame/item position.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamRecord {
    /// Session-local charged frame ordinal; unrelated to a provider sequence.
    pub frame: EvidenceRevision,
    /// Zero-based position within that frame; gaps after selection are allowed.
    pub item: u32,
    /// Source observation identity/revision.
    pub observation: EvidenceVersion,
    /// Explicit predecessor, null only for revision one.
    #[serde(deserialize_with = "super::model::required")]
    pub predecessor: Option<EvidenceRevision>,
    /// Exact normalization revision.
    pub transformation: EvidenceRevision,
    /// Actual source sequence, preserving zero; never filled from local ordinals.
    #[serde(deserialize_with = "super::model::required")]
    pub sequence: Option<EvidenceSequence>,
    /// Source event instant.
    pub event_at: EvidenceTime,
    /// Actual local receipt.
    pub received_at: EvidenceTime,
    /// Optional provider observation time.
    #[serde(deserialize_with = "super::model::required")]
    pub observed_at: Option<EvidenceTime>,
    /// Optional provider publication time.
    #[serde(deserialize_with = "super::model::required")]
    pub published_at: Option<EvidenceTime>,
    /// Supplied local session label, never inferred from UTC.
    pub date: EvidenceDate,
    /// Sorted, distinct, qualified record-level codes.
    pub conditions: Vec<u16>,
    /// Explicit cancellation is null and must name a predecessor.
    #[serde(deserialize_with = "super::model::required")]
    pub value: Option<StreamValue>,
}
/// Untrusted wire input. Seal through `StreamSegment::new` or bounded `decode`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamSegmentData {
    /// Exact streaming format.
    pub version: StreamSegmentFormat,
    /// Explicit immutable operator bounds, independent of frame/account budgets.
    pub limits: StreamSegmentLimits,
    /// Full licensed session binding.
    pub scope: StreamSegmentScope,
    /// Exact source semantic catalog revision.
    pub semantics: EvidenceVersion,
    /// Declared source delivery, not proof of completeness.
    pub delivery: StreamDelivery,
    /// Positive feed epoch; resets require a new explicit epoch.
    pub epoch: EvidenceRevision,
    /// Durable connection generation; not authentication or a lease.
    pub generation: EvidenceRevision,
    /// Inclusive charged-frame interval, including controls/filtered records.
    pub frames: [EvidenceRevision; 2],
    /// Prior segment digest, null only for an explicitly unlinked first segment.
    #[serde(deserialize_with = "super::model::required")]
    pub previous: Option<EvidenceDigest>,
    /// Actual sealing time, not source availability.
    pub captured_at: EvidenceTime,
    /// Exclusive licensed deadline; never derived from a new retry time.
    pub delete_by: EvidenceTime,
    /// Ordered frame/item positions; may be empty for control-only intervals.
    pub records: Vec<StreamRecord>,
}
/// Sealed exact canonical bytes; storage/publication still require current authority.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamSegment {
    data: StreamSegmentData,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
impl StreamSegment {
    /// Validate wire invariants; domain authority and global history are producer obligations.
    /// # Errors
    /// Rejects invalid, oversized or inconsistent input without payload diagnostics.
    pub fn new(data: StreamSegmentData) -> Result<Self, EvidenceContractError> {
        validate::segment(&data)?;
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&data).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > data.limits.bytes as usize {
            return Err(EvidenceContractError);
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        if data.previous.as_ref() == Some(&digest) {
            return Err(EvidenceContractError);
        }
        Ok(Self {
            data,
            bytes,
            digest,
        })
    }
    /// Bound before hashing/parsing, verify identity and reject alternate encodings.
    /// # Errors
    /// Rejects unsupported, noncanonical, corrupt or oversized bytes.
    pub fn decode(bytes: &[u8], expected: &EvidenceDigest) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_STREAM_SEGMENT_BYTES || &DailyEvidenceArtifact::hash(bytes) != expected
        {
            return Err(EvidenceContractError);
        }
        let data = serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?;
        let result = Self::new(data)?;
        if result.bytes != bytes {
            return Err(EvidenceContractError);
        }
        Ok(result)
    }
    /// Borrow immutable checked wire fields.
    #[must_use]
    pub const fn data(&self) -> &StreamSegmentData {
        &self.data
    }
    /// Exact bytes for storage; do not log them or reserialize on retries.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content identity, never a read capability.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
/// Owner-private descriptor. Neither the checksum nor the namespace grants access.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamSegmentObject {
    /// Complete licensed scope and exact finite admission binding.
    pub scope: StreamSegmentScope,
    /// Exact canonical content identity.
    pub sha256: EvidenceDigest,
    /// Bounded canonical length, including final LF.
    pub bytes: u32,
    /// Original exclusive licensed deadline.
    pub delete_by: EvidenceTime,
}
impl StreamSegment {
    /// Exact expected descriptor for create/read-back or later atomic publication.
    #[must_use]
    pub fn object(&self) -> StreamSegmentObject {
        StreamSegmentObject {
            scope: self.data.scope.clone(),
            sha256: self.digest.clone(),
            bytes: self.bytes.len() as u32,
            delete_by: self.data.delete_by,
        }
    }
}
redacted!(StreamSegmentObject);
redacted!(
    StreamSegmentScope,
    StreamQuoteSide,
    StreamValue,
    StreamRecord,
    StreamSegmentData,
    StreamSegment
);
mod validate;
