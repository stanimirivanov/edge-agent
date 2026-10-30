//! Owner-exported policy companion. Canonical bytes and hashes do not grant read authority.
use crate::{
    evidence::*,
    research::{ResearchRecord, ResearchRequest},
};
use serde::{Deserialize, Serialize};
/// Maximum companion size, including its retained daily object and bounded event census.
pub const MAX_POLICY_BYTES: usize = MAX_EVIDENCE_BYTES + 65_536;
/// Unknown coverage is distinct from an affirmative, explicitly invented event calendar.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventCoverage {
    Unknown,
    /// Complete invented coverage over a half-open Unix-millisecond interval.
    SyntheticComplete {
        revision: EvidenceDigest,
        known_at_ms: i64,
        interval: [i64; 2],
        events: Vec<i64>,
    },
}
/// Versioned owner companion; the daily object preserves exact source, units and quality lineage.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum PolicyEvidence {
    #[serde(rename = "research.policy-evidence.v1")]
    V1 {
        request: ResearchRequest,
        daily: Box<DailyEvidenceArtifact>,
        events: EventCoverage,
    },
}
fn ms(t: EvidenceTime) -> Result<i64, EvidenceContractError> {
    if !t.nanos.is_multiple_of(1_000_000) {
        return Err(EvidenceContractError);
    }
    t.seconds
        .checked_mul(1000)
        .and_then(|v| v.checked_add(i64::from(t.nanos / 1_000_000)))
        .ok_or(EvidenceContractError)
}
impl PolicyEvidence {
    /// Validate chronology and scope, then encode canonical keys and final LF.
    pub fn bytes(&self) -> Result<Vec<u8>, EvidenceContractError> {
        let Self::V1 {
            request,
            daily,
            events,
        } = self;
        ResearchRecord::Request(request.clone())
            .canonical_bytes()
            .map_err(|_| EvidenceContractError)?;
        let data = daily.data();
        let cutoff = request.decision_at_ms;
        if request.instruments != [data.scope.instrument.as_str()]
            || ms(data.captured_at)? > cutoff
            || ms(data.delete_by)? <= cutoff
        {
            return Err(EvidenceContractError);
        }
        for r in &data.references {
            if ms(r.known_at)? > cutoff {
                return Err(EvidenceContractError);
            }
        }
        for b in &data.bars {
            if ms(b.event_at)? > cutoff || ms(b.received_at)? > cutoff {
                return Err(EvidenceContractError);
            }
        }
        if let EventCoverage::SyntheticComplete {
            known_at_ms,
            interval,
            events,
            ..
        } = events
            && (*known_at_ms < 0
                || *known_at_ms > cutoff
                || interval[0] < 0
                || interval[0] > cutoff
                || interval[1] <= cutoff
                || interval[1] > ms(data.delete_by)?
                || events.len() > 256
                || !events.windows(2).all(|w| w[0] < w[1])
                || events.iter().any(|t| *t < interval[0] || *t >= interval[1]))
        {
            return Err(EvidenceContractError);
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(self).map_err(|_| EvidenceContractError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(EvidenceContractError);
        }
        Ok(bytes)
    }
    /// Decode only exact bounded canonical bytes; the caller authenticates the owner process.
    pub fn decode(bytes: &[u8]) -> Result<Self, EvidenceContractError> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(EvidenceContractError);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| EvidenceContractError)?;
        if value.bytes()? != bytes {
            return Err(EvidenceContractError);
        }
        Ok(value)
    }
    /// Bind an authenticated companion to the exact original request and retained daily digest.
    pub fn bind(
        &self,
        request: &ResearchRequest,
        digest: &EvidenceDigest,
    ) -> Result<(), EvidenceContractError> {
        let Self::V1 {
            request: retained,
            daily,
            ..
        } = self;
        self.bytes()?;
        if retained != request || daily.digest() != digest {
            return Err(EvidenceContractError);
        }
        Ok(())
    }
}
