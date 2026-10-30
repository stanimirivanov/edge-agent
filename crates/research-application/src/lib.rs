//! Pure request-to-outcome composition; no I/O, authentication or publication.
//! The caller must authenticate owner/audience assertions and verify retained bytes.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
pub mod experiment;
pub mod governed;
pub mod lifecycle;
pub mod publication;
pub mod replay;
pub mod validation;
pub mod worker;

use edgeagent_contracts::research::{
    AbstentionReason, EvidenceRef, RankedCandidate, ResearchAbstention, ResearchArtifact,
    ResearchRecord, ResearchRequest, Unavailable,
};
use edgeagent_research_domain::{Abstention, EvidencePolicy, FrozenSeries, momentum, strategy};
/// Fixed template and application profile revision; implementation/configuration must pin it.
pub const RENDERER_REVISION: &str = "research-ranking-raw-v1";
/// Safe evaluation failure, distinct from an accepted-request evidence abstention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationError {
    /// Malformed versioned request.
    InvalidRequest,
    /// Request differs from selected policy identities, cutoff, universe or lookback.
    BindingMismatch,
    /// Expiry is not after decision or exceeds retained use authorization.
    InvalidExpiry,
    /// No partial ranking is returned after a calculation failure.
    Calculation(momentum::MomentumError),
    /// The internally constructed outcome violates its wire contract.
    InvalidOutcome,
}
impl std::fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "invalid research request",
            Self::BindingMismatch => "research policy binding mismatch",
            Self::InvalidExpiry => "invalid research expiry",
            Self::Calculation(_) => "research calculation failed",
            Self::InvalidOutcome => "invalid research outcome",
        })
    }
}
impl std::error::Error for EvaluationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Calculation(cause) => Some(cause),
            _ => None,
        }
    }
}
/// Immutable validated outcome and its exact canonical bytes/content digest.
pub struct RenderedOutcome {
    record: ResearchRecord,
    bytes: Vec<u8>,
    digest: String,
}
impl RenderedOutcome {
    /// Validated artifact or abstention record; never a mutable wire DTO reference.
    #[must_use]
    pub const fn record(&self) -> &ResearchRecord {
        &self.record
    }
    /// Exact compact key-sorted JSON with one final LF, suitable for later atomic storage.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// SHA-256 of these exact bytes, not an authentication or license claim.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
/// Render an owner/policy-classified no-evidence outcome without inventing market facts.
/// # Errors
/// Rejects invalid request/reason contract combinations.
pub fn abstain(
    request: ResearchRequest,
    reason: AbstentionReason,
) -> Result<RenderedOutcome, EvaluationError> {
    finish(ResearchRecord::Abstention(ResearchAbstention {
        request,
        reason,
    }))
}
fn finish(record: ResearchRecord) -> Result<RenderedOutcome, EvaluationError> {
    let bytes = record
        .canonical_bytes()
        .map_err(|_| EvaluationError::InvalidOutcome)?;
    let digest = record
        .digest()
        .map_err(|_| EvaluationError::InvalidOutcome)?;
    Ok(RenderedOutcome {
        record,
        bytes,
        digest,
    })
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn reason(value: Abstention) -> AbstentionReason {
    match value {
        Abstention::MissingEntitlement => AbstentionReason::MissingEntitlement,
        Abstention::UnsupportedUniverse => AbstentionReason::UnsupportedUniverse,
        Abstention::InvalidEvidence => AbstentionReason::InvalidEvidence,
        Abstention::FutureEvidence => AbstentionReason::FutureEvidence,
        Abstention::StaleEvidence => AbstentionReason::StaleEvidence,
        Abstention::InsufficientHistory => AbstentionReason::InsufficientHistory,
    }
}
/// Evaluate one admitted request using already authenticated owner evidence assertions.
/// Application-profile instrument tokens are canonical 32-character lowercase hex IDs.
/// Every evidence-qualified candidate is ranked, including zero/negative returns. Failed
/// required evidence abstains for the whole request; no partial or silently filtered universe.
/// # Errors
/// Rejects malformed/mismatched requests, invalid expiry, unrepresentable scores or outcomes.
pub fn evaluate(
    request: ResearchRequest,
    policy: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
) -> Result<RenderedOutcome, EvaluationError> {
    // Move, rather than clone, raw input before its existing contract validates bounds.
    let raw = ResearchRecord::Request(request);
    raw.canonical_bytes()
        .map_err(|_| EvaluationError::InvalidRequest)?;
    let request = match raw {
        ResearchRecord::Request(r) => r,
        _ => return Err(EvaluationError::InvalidRequest),
    };
    let binding = policy.binding();
    let universe: Vec<_> = policy
        .universe()
        .iter()
        .map(|s| format!("{:032x}", s.instrument.get()))
        .collect();
    if request.decision_at_ms != binding.decision_at_ms
        || request.implementation != hex(&binding.implementation)
        || request.configuration != hex(&binding.configuration)
        || request.instruments != universe
        || usize::from(request.lookback_sessions) + 1 != policy.sessions().len()
    {
        return Err(EvaluationError::BindingMismatch);
    }
    if expires_at_ms <= binding.decision_at_ms {
        return Err(EvaluationError::InvalidExpiry);
    }
    let eligible = match policy.assess(inputs) {
        Ok(value) => value,
        Err(value) => {
            return finish(ResearchRecord::Abstention(ResearchAbstention {
                request,
                reason: reason(value),
            }));
        }
    };
    if eligible
        .series()
        .iter()
        .any(|s| expires_at_ms > s.frozen().rights_until_ms)
    {
        return Err(EvaluationError::InvalidExpiry);
    }
    let selected_strategy = strategy::MOMENTUM;
    let ranking = selected_strategy
        .rank(&eligible)
        .map_err(EvaluationError::Calculation)?
        .into_iter()
        .map(|c| RankedCandidate {
            instrument: format!("{:032x}", c.instrument().get()),
            score_ppm: c.momentum().score_ppm(),
        })
        .collect();
    let mut evidence: Vec<_> = eligible
        .series()
        .iter()
        .map(|s| EvidenceRef {
            instrument: format!("{:032x}", s.frozen().selected.instrument.get()),
            digest: hex(&s.frozen().evidence),
            available_at_ms: s
                .closes()
                .iter()
                .map(|c| c.available_at_ms)
                .max()
                .unwrap_or(s.frozen().known_at_ms),
        })
        .collect();
    evidence.sort_by(|a, b| (&a.instrument, &a.digest).cmp(&(&b.instrument, &b.digest)));
    let explanation = format!(
        "{RENDERER_REVISION}; {}; lookback_sessions={}; score_unit=ppm; basis=raw_usd; scenarios=not_qualified.",
        selected_strategy.revision(),
        request.lookback_sessions
    );
    finish(ResearchRecord::Artifact(ResearchArtifact {
        request,
        ranking,
        evidence,
        expires_at_ms,
        explanation,
        scenarios: Unavailable::NotQualified,
    }))
}
