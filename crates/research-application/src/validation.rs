//! Reconcile untrusted proposals with a fresh deterministic evaluation of trusted owner inputs.
use crate::{
    RenderedOutcome,
    governed::{self, ContextInputs, GovernedOutcome},
};
use edgeagent_contracts::research::{ResearchContractError, ResearchRecord, ResearchRequest};
use edgeagent_research_domain::{
    EvidencePolicy, FrozenSeries,
    policy::{Decision, Intent, Policy},
};
/// Exact canonical reconciliation revision; not an authentication or approval claim.
pub const REVISION: &str = "reconcile-governed-research-exact-v1";
/// Selected owner-authenticated inputs, never populated from the proposal's claims.
pub struct Inputs<'a> {
    /// Exact originally admitted request.
    pub request: ResearchRequest,
    /// Trusted historical universe/session and evidence policy.
    pub evidence: &'a EvidencePolicy,
    /// Authorized and integrity-checked retained assertions.
    pub series: &'a [FrozenSeries],
    /// Explicit immutable profile expiry.
    pub expires_at_ms: i64,
    /// Independently selected immutable eligibility policy.
    pub policy: &'a Policy,
    /// Explicit trusted direction and holding horizon.
    pub intent: Intent,
    /// Owner-bound inputs for every selected constraint.
    pub context: ContextInputs<'a>,
}
/// Payload-safe reconciliation failure; never formats differences or raw proposal bytes.
#[derive(Debug)]
pub enum Error {
    /// Invalid, oversized or noncanonical proposal.
    Proposal(ResearchContractError),
    /// A request or lifecycle record is not a terminal proposal.
    NotTerminal,
    /// Trusted inputs could not produce a qualified governed result.
    Evaluation(governed::Error),
    /// Canonical proposal differs from independently rebuilt facts or policy outcome.
    Mismatch,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("research proposal validation refused")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Proposal(e) => Some(e),
            Self::Evaluation(e) => Some(e),
            Self::NotTerminal | Self::Mismatch => None,
        }
    }
}
/// Sealed independently reconciled terminal result. No public constructor or conversion exists.
/// ```compile_fail
/// use edgeagent_contracts::{MessageEnvelope,research::ResearchRecord};
/// use edgeagent_research_application::publication::PreparedCompletion;
/// fn bypass(delivery:MessageEnvelope,proposal:&ResearchRecord) {
///     let _=PreparedCompletion::from_validated(delivery,proposal,"2026-10-30T00:00:00Z");
/// }
/// ```
pub struct ValidatedOutcome {
    governed: GovernedOutcome,
}
impl ValidatedOutcome {
    /// Reconciled canonical artifact or abstention, exposed immutably.
    #[must_use]
    pub fn record(&self) -> &ResearchRecord {
        self.governed.outcome().record()
    }
    /// Exact canonical bytes; no rewriting or tolerance-based repair.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.governed.outcome().bytes()
    }
    /// Content identity of the exact reconciled terminal bytes.
    #[must_use]
    pub fn digest(&self) -> &str {
        self.governed.outcome().digest()
    }
    /// Independently computed decision; absent for critical evidence abstentions.
    #[must_use]
    pub fn decision(&self) -> Option<&Decision> {
        self.governed.decision()
    }
    pub(crate) fn rendered(&self) -> &RenderedOutcome {
        self.governed.outcome()
    }
}
/// Decode a bounded canonical proposal, rebuild its expected outcome and reconcile every byte.
/// The proposal supplies no policy, evidence, expiry or approval authority. Integer scores
/// must match exactly: their existing rounding already encodes the qualified tolerance.
/// # Errors
/// Rejects invalid/nonterminal proposals, failed trusted evaluation and any mismatch.
pub fn validate(proposal: &[u8], inputs: Inputs<'_>) -> Result<ValidatedOutcome, Error> {
    let decoded = ResearchRecord::decode(proposal).map_err(Error::Proposal)?;
    if !matches!(
        decoded,
        ResearchRecord::Artifact(_) | ResearchRecord::Abstention(_)
    ) {
        return Err(Error::NotTerminal);
    }
    let rebuilt = governed::evaluate_with_context_inputs(
        inputs.request,
        inputs.evidence,
        inputs.series,
        inputs.expires_at_ms,
        inputs.policy,
        inputs.intent,
        inputs.context,
    )
    .map_err(Error::Evaluation)?;
    if rebuilt.outcome().bytes() != proposal {
        return Err(Error::Mismatch);
    }
    Ok(ValidatedOutcome { governed: rebuilt })
}

/// Validate the deterministic terminal rendering of a separately trusted negative owner fact.
/// No numeric/evidence success is asserted, and no caller-supplied positive proposal is accepted.
pub(crate) fn refused(
    request: ResearchRequest,
    reason: edgeagent_contracts::research::AbstentionReason,
) -> Result<ValidatedOutcome, Error> {
    let governed = governed::refused(request, reason).map_err(Error::Evaluation)?;
    ResearchRecord::decode(governed.outcome().bytes()).map_err(Error::Proposal)?;
    Ok(ValidatedOutcome { governed })
}
