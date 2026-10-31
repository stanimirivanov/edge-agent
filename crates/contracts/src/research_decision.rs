//! Immutable companion observations. Shape/integrity never confer owner authority.
use crate::research::{AbstentionReason, ResearchRecord, ResearchRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Payload-safe refusal at the decision-provenance boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionContractError;
impl std::fmt::Display for DecisionContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research decision v1 contract")
    }
}
impl std::error::Error for DecisionContractError {}
use DecisionContractError as Error;
/// Bound before parsing; independent of legacy Research v1 size limits.
pub const MAX_DECISION_BYTES: usize = 16_384;
/// Explicit intent, separate from historical lookback and artifact expiry.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Long,
    Short,
}
/// Stable eligibility reason order, matching the deterministic policy revision.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    InstrumentNotAllowed,
    DirectionNotAllowed,
    HorizonExceeded,
    LiquidityBelowMinimum,
    VolatilityExceeded,
    GapExceeded,
    EventExcluded,
    RegimeNotAllowed,
}
/// Eligibility only, never strategy approval or account-risk authorization.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Approved,
    Narrowed,
    Abstained,
}
/// Critical evidence failures must not manufacture an evaluated policy decision.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "assessment", rename_all = "snake_case", deny_unknown_fields)]
pub enum Assessment {
    Evaluated {
        status: Status,
        eligible: Vec<String>,
        reasons: Vec<Reason>,
    },
    NotEvaluated {
        reason: AbstentionReason,
    },
}
/// Required fields have no inferred legacy defaults. No payload-bearing Debug.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecisionProvenance {
    pub request: ResearchRequest,
    /// Digest of exact canonical artifact or abstention bytes.
    pub outcome: String,
    pub policy: String,
    pub direction: Direction,
    pub horizon_sessions: u16,
    /// Digest of immutable owner approval profile, not a caller approval Boolean.
    pub approval_profile: String,
    /// Exact observed owner approval revision; freshness is checked separately.
    pub approval_revision: u64,
    pub validator: String,
    pub assessment: Assessment,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum Versioned<T> {
    #[serde(rename = "research.decision.v1")]
    V1(T),
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().any(|b| b != b'0')
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl DecisionProvenance {
    /// Validate shape and emit key-sorted JSON with one final newline.
    /// # Errors
    /// Rejects malformed identities, contradictory eligibility and oversized records.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        ResearchRecord::Request(self.request.clone())
            .canonical_bytes()
            .map_err(|_| Error)?;
        let assessed = match &self.assessment {
            Assessment::NotEvaluated { reason } => *reason != AbstentionReason::NoEligibleCandidate,
            Assessment::Evaluated {
                status,
                eligible,
                reasons,
            } => {
                eligible.windows(2).all(|w| w[0] < w[1])
                    && eligible
                        .iter()
                        .all(|v| self.request.instruments.contains(v))
                    && reasons.windows(2).all(|w| w[0] < w[1])
                    && match status {
                        Status::Approved => {
                            eligible == &self.request.instruments && reasons.is_empty()
                        }
                        Status::Narrowed => {
                            !eligible.is_empty()
                                && eligible.len() < self.request.instruments.len()
                                && !reasons.is_empty()
                        }
                        Status::Abstained => eligible.is_empty() && !reasons.is_empty(),
                    }
            }
        };
        if !assessed
            || !digest(&self.outcome)
            || !digest(&self.policy)
            || !digest(&self.approval_profile)
            || self.approval_revision == 0
            || !(1..=252).contains(&self.horizon_sessions)
            || self.validator.is_empty()
            || self.validator.len() > 64
            || !self
                .validator
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Error);
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(&Versioned::V1(self)).map_err(|_| Error)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_DECISION_BYTES {
            return Err(Error);
        }
        Ok(bytes)
    }
    /// Bounded strict version/field decoding; never repair alternate encodings.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_DECISION_BYTES {
            return Err(Error);
        }
        let Versioned::V1(value): Versioned<Self> =
            serde_json::from_slice(bytes).map_err(|_| Error)?;
        if value.canonical_bytes()? != bytes {
            return Err(Error);
        }
        Ok(value)
    }
    /// Stable integrity identity, not authentication.
    pub fn digest(&self) -> Result<String, Error> {
        Ok(Sha256::digest(self.canonical_bytes()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    /// Bind observations to exact immutable terminal bytes and their eligible set.
    /// This checks consistency, not formulas, approval currency or publication authority.
    pub fn bind(&self, outcome: &ResearchRecord) -> Result<(), Error> {
        self.canonical_bytes()?;
        if self.outcome != outcome.digest().map_err(|_| Error)? {
            return Err(Error);
        }
        let valid = match (outcome, &self.assessment) {
            (
                ResearchRecord::Artifact(a),
                Assessment::Evaluated {
                    status, eligible, ..
                },
            ) => {
                let mut ranked = a
                    .ranking
                    .iter()
                    .map(|v| v.instrument.clone())
                    .collect::<Vec<_>>();
                ranked.sort();
                a.request == self.request && *status != Status::Abstained && &ranked == eligible
            }
            (ResearchRecord::Abstention(a), Assessment::NotEvaluated { reason }) => {
                a.request == self.request && a.reason == *reason
            }
            (ResearchRecord::Abstention(a), Assessment::Evaluated { status, .. }) => {
                a.request == self.request
                    && *status == Status::Abstained
                    && a.reason == AbstentionReason::NoEligibleCandidate
            }
            _ => false,
        };
        if valid { Ok(()) } else { Err(Error) }
    }
}
