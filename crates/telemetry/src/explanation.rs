//! Closed explanation observations without evidence, content or business identifiers.
use std::time::Duration;

/// Completed explanation operations by bounded stage and outcome.
pub const EXPLANATION_OPERATIONS: &str = "edgeagent.explanation.operations";
/// Elapsed explanation operation time in seconds.
pub const EXPLANATION_DURATION: &str = "edgeagent.explanation.operation.duration";

/// The boundary observed by an explanation signal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplanationStage {
    /// A bounded model port call, before independent semantic validation.
    Generation,
    /// Local input and allowlist validation.
    Validation,
    /// The result of a durable job invocation, including recovery or idle work.
    Completion,
}
impl ExplanationStage {
    /// Complete vocabulary used by the closed projection.
    pub const ALL: [Self; 3] = [Self::Generation, Self::Validation, Self::Completion];
    /// Stable metric and trace dimension.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generation => "generation",
            Self::Validation => "validation",
            Self::Completion => "completion",
        }
    }
}
/// Fixed classifications. A generation proposal is not a validated completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplanationOutcome {
    /// The model port returned a bounded proposal.
    ProposalReceived,
    /// Input and allowlist preparation succeeded.
    Validated,
    /// A generated companion was committed.
    Generated,
    /// Generation was disabled and deterministic output was committed.
    Deterministic,
    /// Model availability caused fallback.
    Unavailable,
    /// A model deadline caused fallback.
    Timeout,
    /// The attempt budget caused fallback.
    BudgetExhausted,
    /// Invalid proposal or wire input caused fallback.
    InvalidProposal,
    /// A spent, uncertain attempt recovered through fallback.
    UncertainAttempt,
    /// Serving disclosure was denied.
    DisclosureDenied,
    /// Current authority could not be established.
    AuthorityUnavailable,
    /// An existing terminal receipt was recovered.
    Recovered,
    /// A spent attempt is still inside its deadline.
    Pending,
    /// No unfinished job was available.
    Idle,
    /// An operation failed without a confirmed terminal result.
    Failed,
}
impl ExplanationOutcome {
    /// Complete vocabulary used by the closed projection.
    pub const ALL: [Self; 15] = [
        Self::ProposalReceived,
        Self::Validated,
        Self::Generated,
        Self::Deterministic,
        Self::Unavailable,
        Self::Timeout,
        Self::BudgetExhausted,
        Self::InvalidProposal,
        Self::UncertainAttempt,
        Self::DisclosureDenied,
        Self::AuthorityUnavailable,
        Self::Recovered,
        Self::Pending,
        Self::Idle,
        Self::Failed,
    ];
    /// Stable metric and trace dimension.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProposalReceived => "proposal_received",
            Self::Validated => "validated",
            Self::Generated => "generated",
            Self::Deterministic => "deterministic",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::BudgetExhausted => "budget_exhausted",
            Self::InvalidProposal => "invalid_proposal",
            Self::UncertainAttempt => "uncertain_attempt",
            Self::DisclosureDenied => "disclosure_denied",
            Self::AuthorityUnavailable => "authority_unavailable",
            Self::Recovered => "recovered",
            Self::Pending => "pending",
            Self::Idle => "idle",
            Self::Failed => "failed",
        }
    }
}
/// Record a completed operation with a closed, content-free vocabulary.
pub fn record_explanation_operation(
    stage: ExplanationStage,
    outcome: ExplanationOutcome,
    duration: Duration,
) {
    metrics::counter!(EXPLANATION_OPERATIONS,"stage"=>stage.as_str(),"outcome"=>outcome.as_str())
        .increment(1);
    metrics::histogram!(EXPLANATION_DURATION,"stage"=>stage.as_str(),"outcome"=>outcome.as_str())
        .record(duration.as_secs_f64());
    tracing::event!(target:"edgeagent.explanation",tracing::Level::INFO,
        event_name="edgeagent.explanation.operation",stage=stage.as_str(),outcome=outcome.as_str(),duration_seconds=duration.as_secs_f64());
}
