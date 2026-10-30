//! Isolated experiments: deterministic results without store, approval or publication ports.
use crate::{EvaluationError, evaluate as evaluate_research};
use edgeagent_contracts::research::ResearchRequest;
use edgeagent_research_domain::{EvidencePolicy, FrozenSeries};

/// Explicitly non-approved experiment bytes. There is no conversion to `RenderedOutcome`.
/// Publication requires a different type, even when numerical results are identical.
/// ```compile_fail
/// use edgeagent_contracts::MessageEnvelope;
/// use edgeagent_research_application::{experiment::ExperimentalOutcome, publication::PreparedCompletion};
/// fn publish(delivery: MessageEnvelope, experiment: &ExperimentalOutcome) {
///     let _ = PreparedCompletion::new(delivery, experiment, "2026-10-30T00:00:00Z");
/// }
/// ```
pub struct ExperimentalOutcome {
    bytes: Vec<u8>,
}
impl ExperimentalOutcome {
    /// Canonical experiment envelope with `publication_eligible:false`, never a ResearchRecord.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
/// Evaluate exact M04 profile bindings with the same evidence, ranking and rendering rules.
/// No approval is asserted, and no persistence, publication or simulation capability is accepted.
/// # Errors
/// Preserves normal request/binding, evidence-expiry and numeric evaluation errors.
pub fn evaluate(
    request: ResearchRequest,
    policy: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
) -> Result<ExperimentalOutcome, EvaluationError> {
    let result = evaluate_research(request, policy, inputs, expires_at_ms)?;
    // Keys are already canonical; the nested record is independently canonical and bounded.
    // Keep that record private so callers cannot accidentally submit it for publication.
    let mut bytes =
        br#"{"kind":"research.experiment.v1","publication_eligible":false,"result":"#.to_vec();
    let nested = result
        .bytes()
        .strip_suffix(b"\n")
        .ok_or(EvaluationError::InvalidOutcome)?;
    bytes.extend_from_slice(nested);
    bytes.extend_from_slice(b"}\n");
    Ok(ExperimentalOutcome { bytes })
}
