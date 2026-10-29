//! Research v1 payloads, not strategy policy or activated message routes.
//! Use [`ResearchRecord::decode`] at untrusted byte boundaries.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[cfg(feature = "schema")]
pub mod schemas;
/// Maximum encoded research payload, leaving room for its message envelope.
pub const MAX_RESEARCH_BYTES: usize = 131_072;
/// Maximum explicitly selected instruments/evidence references.
pub const MAX_CANDIDATES: usize = 64;
/// Payload-safe rejection; raw parser diagnostics never cross this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResearchContractError;
impl std::fmt::Display for ResearchContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research v1 contract")
    }
}
impl std::error::Error for ResearchContractError {}

macro_rules! dto {
    ($(#[$meta:meta])* $name:ident { $($(#[$field:meta])* $key:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$field])* pub $key: $ty),* }
    };
}
/// Stable v1 terminal abstention vocabulary; additions require compatibility review.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AbstentionReason {
    /// No complete lookback.
    InsufficientHistory,
    /// Evidence was unavailable at decision time.
    FutureEvidence,
    /// Evidence exceeded the selected freshness policy.
    StaleEvidence,
    /// Conflicting or corrupt evidence.
    InvalidEvidence,
    /// Selected instruments cannot be evaluated by this implementation.
    UnsupportedUniverse,
    /// Evidence use is not authorized.
    MissingEntitlement,
    /// No candidate passes the selected minimum policy.
    NoEligibleCandidate,
}
dto! {
    /// Owner-authenticated immutable evidence reference; never a fetch URL.
    EvidenceRef {
        /// Stable instrument token.
        instrument: String,
        /// SHA-256 of exact retained evidence bytes.
        digest: String,
        /// Evidence availability in Unix milliseconds, including processing delay.
        available_at_ms: i64,
    }
}
dto! {
    /// Bounded research intent; policy configuration is identified, not interpreted here.
    ResearchRequest {
        /// Caller-scoped stable request token; not a CloudEvents delivery ID.
        request_id: String,
        /// Explicit caller/tenant scope supplied by authenticated ingress.
        scope: String,
        /// Immutable implementation revision digest.
        implementation: String,
        /// Immutable strategy/numeric/evidence policy configuration digest.
        configuration: String,
        /// Point-in-time decision cutoff, Unix milliseconds.
        decision_at_ms: i64,
        /// Lookback in trading sessions; not calendar days or holding horizon.
        lookback_sessions: u16,
        /// Explicit historical universe, in stable instrument-token order.
        instruments: Vec<String>,
    }
}
dto! {
    /// One ranked candidate; score is a dimensionless return in parts per million.
    RankedCandidate {
        /// Instrument from the requested universe.
        instrument: String,
        /// Checked, policy-rounded momentum score; no floating-point JSON.
        score_ppm: i64,
    }
}
dto! {
    /// Immutable deterministic result. Scenario/risk outputs are explicitly unavailable in v1.
    ResearchArtifact {
        /// Exact admitted request and its pinned policy identities.
        request: ResearchRequest,
        /// Ordered eligible candidates; score descending, instrument ascending on ties.
        ranking: Vec<RankedCandidate>,
        /// Complete retained evidence census, sorted by instrument and digest.
        evidence: Vec<EvidenceRef>,
        /// Expiry cutoff, Unix milliseconds; later expiry is an appended lifecycle event.
        expires_at_ms: i64,
        /// Bounded deterministic explanation; never establishes facts or policy authority.
        explanation: String,
        /// Explicit unavailable value; no invented entry/target/risk illustrations.
        scenarios: Unavailable,
    }
}
/// Explicit unavailable scenario/risk output; null, omission and numeric zero are different.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// Requires a separately qualified scenario/risk contract.
    NotQualified,
}
dto! {
    /// Accepted request with a terminal evidence-policy abstention, not an ingress rejection.
    ResearchAbstention {
        /// Exact admitted request.
        request: ResearchRequest,
        /// Stable reason, without private provider diagnostics.
        reason: AbstentionReason,
    }
}
/// Append-only lifecycle change; validation of prior owner state belongs to the application.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "transition", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleChange {
    /// Owner invalidated the artifact.
    Invalidated,
    /// Owner withdrew the artifact.
    Withdrawn,
    /// The explicit artifact expiry was reached.
    Expired,
    /// A separately committed artifact replaces this one.
    Corrected {
        /// SHA-256 of the replacement artifact record.
        replacement: String,
    },
}
dto! {
    /// Owner-authenticated lifecycle fact; not public command authority.
    ResearchLifecycle {
        /// Caller scope.
        scope: String,
        /// Request identity.
        request_id: String,
        /// SHA-256 of immutable original artifact record bytes.
        artifact: String,
        /// Consecutive owner revision, starting at one.
        revision: u64,
        /// Effective Unix milliseconds; application verifies against prior state.
        effective_at_ms: i64,
        /// Explicit owner transition.
        change: LifecycleChange,
    }
}
/// Versioned record union. Raw DTO deserialization alone does not validate semantics.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum ResearchRecord {
    /// Request v1.
    #[serde(rename = "research.request.v1")]
    Request(ResearchRequest),
    /// Artifact v1.
    #[serde(rename = "research.artifact.v1")]
    Artifact(ResearchArtifact),
    /// Abstention v1.
    #[serde(rename = "research.abstention.v1")]
    Abstention(ResearchAbstention),
    /// Lifecycle fact v1.
    #[serde(rename = "research.lifecycle.v1")]
    Lifecycle(ResearchLifecycle),
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn request(r: &ResearchRequest) -> bool {
    token(&r.request_id)
        && token(&r.scope)
        && digest(&r.implementation)
        && digest(&r.configuration)
        && r.decision_at_ms >= 0
        && (1..=252).contains(&r.lookback_sessions)
        && !r.instruments.is_empty()
        && r.instruments.len() <= MAX_CANDIDATES
        && r.instruments.iter().all(|v| token(v))
        && r.instruments.windows(2).all(|w| w[0] < w[1])
}
impl ResearchRecord {
    /// Validate semantics and return stable, recursively key-sorted JSON bytes.
    /// Array order is meaningful; noncanonical bytes are never silently repaired on decode.
    /// # Errors
    /// Rejects malformed, contradictory or oversized records without payload diagnostics.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ResearchContractError> {
        let valid = match self {
            Self::Request(r) => request(r),
            Self::Abstention(a) => request(&a.request),
            Self::Lifecycle(l) => {
                token(&l.scope)
                    && token(&l.request_id)
                    && digest(&l.artifact)
                    && l.revision > 0
                    && l.effective_at_ms >= 0
                    && match &l.change {
                        LifecycleChange::Corrected { replacement } => {
                            digest(replacement) && replacement != &l.artifact
                        }
                        _ => true,
                    }
            }
            Self::Artifact(a) => {
                let mut seen = BTreeSet::new();
                request(&a.request)
                    && a.expires_at_ms > a.request.decision_at_ms
                    && !a.explanation.is_empty()
                    && a.explanation.len() <= 4096
                    && !a.explanation.chars().any(char::is_control)
                    && !a.ranking.is_empty()
                    && a.ranking.len() <= MAX_CANDIDATES
                    && a.ranking.iter().all(|c| {
                        a.request.instruments.contains(&c.instrument) && seen.insert(&c.instrument)
                    })
                    && a.ranking.windows(2).all(|w| {
                        w[0].score_ppm > w[1].score_ppm
                            || (w[0].score_ppm == w[1].score_ppm
                                && w[0].instrument < w[1].instrument)
                    })
                    && !a.evidence.is_empty()
                    && a.evidence.len() <= MAX_CANDIDATES
                    && a.evidence.iter().all(|e| {
                        a.request.instruments.contains(&e.instrument)
                            && digest(&e.digest)
                            && e.available_at_ms >= 0
                            && e.available_at_ms <= a.request.decision_at_ms
                    })
                    && a.evidence.windows(2).all(|w| {
                        (&w[0].instrument, &w[0].digest) < (&w[1].instrument, &w[1].digest)
                    })
                    && a.ranking
                        .iter()
                        .all(|c| a.evidence.iter().any(|e| e.instrument == c.instrument))
            }
        };
        if !valid {
            return Err(ResearchContractError);
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(self).map_err(|_| ResearchContractError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RESEARCH_BYTES {
            return Err(ResearchContractError);
        }
        Ok(bytes)
    }
    /// Bound before parsing, validate and require exact canonical bytes.
    /// # Errors
    /// Rejects unknown fields/versions, duplicate keys, alternate encodings and invalid values.
    pub fn decode(bytes: &[u8]) -> Result<Self, ResearchContractError> {
        if bytes.len() > MAX_RESEARCH_BYTES {
            return Err(ResearchContractError);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| ResearchContractError)?;
        if value.canonical_bytes()? != bytes {
            return Err(ResearchContractError);
        }
        Ok(value)
    }
    /// Content digest, independent of delivery ID; not authentication or authorization.
    /// # Errors
    /// Rejects any record that cannot be canonically encoded.
    pub fn digest(&self) -> Result<String, ResearchContractError> {
        Ok(Sha256::digest(self.canonical_bytes()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}
