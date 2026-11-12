//! Deterministic, read-only projection of bound Research records, never disclosure authority.
use crate::{
    explanation::{Anchor, Error},
    messaging::canonical_json,
    research::{EvidenceRef, ResearchRecord, ResearchRequest, Unavailable},
    research_decision::{Assessment, DecisionProvenance, DecisionRecord},
};
use serde::Serialize;

/// Revision of the projection, including score units and unavailable-field semantics.
pub const REVISION: &str = "research.explanation-facts.v1";
/// Bound the projection separately from the original artifact's free-form explanation.
pub const MAX_FACTS_BYTES: usize = 65_536;

#[derive(Serialize)]
struct Score {
    unit: &'static str,
    value: i64,
}
#[derive(Serialize)]
struct Rank<'a> {
    instrument: &'a str,
    score: Score,
    evidence: Vec<&'a EvidenceRef>,
}

/// Facts selected only from an exact artifact/decision pair. No model or wire constructor.
///
/// The caller must authenticate the retained completion and independently verify it
/// against authorized owner evidence before treating these facts as verified. This
/// type checks structural consistency, not policy calculations or current rights.
/// Missing event/observation times and policy measurements are deliberately absent;
/// availability time is never relabeled as either event or observation time.
///
/// ```compile_fail
/// use edgeagent_contracts::explanation_facts::Facts;
/// // Model output cannot be deserialized into the sealed projection.
/// let facts: Facts<'_> = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Serialize)]
pub struct Facts<'a> {
    kind: &'static str,
    anchor: Anchor,
    request: &'a ResearchRequest,
    decision: DecisionRecord<&'a DecisionProvenance>,
    expires_at_ms: i64,
    evidence: &'a [EvidenceRef],
    ranks: Vec<Rank<'a>>,
    scenarios: Unavailable,
}
impl<'a> Facts<'a> {
    /// Project retained intent, policy/approval identities, chronology, and ordered ranks.
    /// Artifact explanation text is excluded: it is not a fact authority.
    /// # Errors
    /// Refuses malformed, foreign, abstained, oversized, or unsupported ranked inputs.
    pub fn from_bound(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
    ) -> Result<Self, Error> {
        decision.bind(record).map_err(|_| Error)?;
        let (ResearchRecord::Artifact(artifact), Assessment::Evaluated { .. }) =
            (record, &decision.assessment)
        else {
            return Err(Error);
        };
        let mut ranks = Vec::with_capacity(artifact.ranking.len());
        for candidate in &artifact.ranking {
            let evidence: Vec<_> = artifact
                .evidence
                .iter()
                .filter(|reference| reference.instrument == candidate.instrument)
                .collect();
            if evidence.is_empty() {
                return Err(Error);
            }
            ranks.push(Rank {
                instrument: &candidate.instrument,
                score: Score {
                    unit: "ppm",
                    value: candidate.score_ppm,
                },
                evidence,
            });
        }
        let facts = Self {
            kind: REVISION,
            anchor: Anchor {
                scope: artifact.request.scope.clone(),
                artifact: decision.outcome.clone(),
                decision: decision.digest().map_err(|_| Error)?,
            },
            request: &artifact.request,
            decision: DecisionRecord::V1(decision),
            expires_at_ms: artifact.expires_at_ms,
            evidence: &artifact.evidence,
            ranks,
            scenarios: artifact.scenarios,
        };
        facts.canonical_bytes()?;
        Ok(facts)
    }

    /// Immutable artifact, decision and scope binding; no authorization assertion.
    pub const fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    /// Exact retained request: universe, cutoff and implementation/configuration pins.
    pub const fn request(&self) -> &ResearchRequest {
        self.request
    }
    /// Exact retained intent, policy assessment and approval bindings.
    pub const fn decision(&self) -> &DecisionProvenance {
        let DecisionRecord::V1(decision) = self.decision;
        decision
    }
    /// Historical artifact expiry; never a current serving authorization.
    pub const fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
    /// Full source census, including references for excluded instruments.
    pub const fn evidence(&self) -> &[EvidenceRef] {
        self.evidence
    }

    /// Recursively key-sorted JSON with one LF, independent of map-order features.
    /// # Errors
    /// Refuses serialization errors or an oversized fact projection.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = canonical_json::to_vec(self).map_err(|_| Error)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FACTS_BYTES {
            return Err(Error);
        }
        Ok(bytes)
    }
}
