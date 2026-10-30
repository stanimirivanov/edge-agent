//! Pure staged eligibility composition. This result does not certify all M05 publication controls.
use crate::{EvaluationError, RenderedOutcome};
use edgeagent_contracts::research::{AbstentionReason, ResearchRecord, ResearchRequest};
use edgeagent_research_domain::{
    EvidencePolicy, FrozenSeries,
    policy::{Decision, Intent, InvalidInput, Policy, Status},
};
/// Governed evaluation failure; accepted-request denials instead return an abstention.
#[derive(Debug)]
pub enum Error {
    /// Existing evaluation failed.
    Evaluation(EvaluationError),
    /// Malformed policy intent.
    Policy(InvalidInput),
    /// Independent policy targets a different configuration.
    Binding,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("governed research evaluation refused")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(e) => Some(e),
            Self::Policy(e) => Some(e),
            Self::Binding => None,
        }
    }
}
/// Result of evidence evaluation plus these independent eligibility controls only.
pub struct GovernedOutcome {
    outcome: RenderedOutcome,
    decision: Option<Decision>,
}
impl GovernedOutcome {
    /// Immutable normal artifact or abstention with existing wire compatibility.
    #[must_use]
    pub const fn outcome(&self) -> &RenderedOutcome {
        &self.outcome
    }
    /// Eligibility decision; absent when required evidence already abstained.
    #[must_use]
    pub const fn decision(&self) -> Option<&Decision> {
        self.decision.as_ref()
    }
}
/// Validate the full required evidence before narrowing; preserve scores and complete lineage.
/// Direction/horizon are explicit trusted intent inputs, never inferred from score sign or TTL.
/// # Errors
/// Rejects malformed intent, configuration mismatch and existing evaluation failures.
pub fn evaluate(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
) -> Result<GovernedOutcome, Error> {
    let expected: String = policy
        .configuration()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if request.configuration != expected {
        return Err(Error::Binding);
    }
    let candidates = evidence
        .universe()
        .iter()
        .map(|s| s.instrument)
        .collect::<Vec<_>>();
    // Validate intent even if evidence will abstain; retain no apparent decision in that case.
    let decision = policy.assess(&candidates, intent).map_err(Error::Policy)?;
    let base = crate::evaluate(request.clone(), evidence, inputs, expires_at_ms)
        .map_err(Error::Evaluation)?;
    let ResearchRecord::Artifact(artifact) = base.record() else {
        return Ok(GovernedOutcome {
            outcome: base,
            decision: None,
        });
    };
    let outcome = match decision.status() {
        Status::Approved => base,
        Status::Abstained => crate::abstain(request, AbstentionReason::NoEligibleCandidate)
            .map_err(Error::Evaluation)?,
        Status::Narrowed => {
            let mut narrowed = artifact.clone();
            narrowed.ranking.retain(|r| {
                decision
                    .eligible()
                    .iter()
                    .any(|id| r.instrument == format!("{:032x}", id.get()))
            });
            crate::finish(ResearchRecord::Artifact(narrowed)).map_err(Error::Evaluation)?
        }
    };
    Ok(GovernedOutcome {
        outcome,
        decision: Some(decision),
    })
}
