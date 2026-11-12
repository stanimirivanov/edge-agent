//! Read-only verification of retained explanation choices; never repeat inference.
#[cfg(test)]
mod tests;
use crate::{
    execution::Selection,
    publication::{CommittedOutcome, decode_decision},
    replay::{ReplayError, verify_execution},
    worker::ExecutionLoader,
};
use crate::{explanation::ValidatedExplanation, explanation_worker::Prepared};
use edgeagent_contracts::MessageEnvelope;
use edgeagent_contracts::{
    explanation::{Companion, Outcome},
    explanation_profile::Profile,
    research::ResearchRecord,
    research_decision::DecisionProvenance,
};

/// Original explanation configuration and authenticated immutable companion.
/// These inputs confer no serving authority.
pub struct Retained<'a> {
    /// Exact original canonical profile; every pin participates in its identity.
    pub profile: &'a Profile,
    /// Exact bytes authenticated against the owner completion and publication.
    pub companion: &'a [u8],
}
/// Independently replay Research with current authorized evidence, then revalidate
/// the retained explanation without accepting a model, completion or publication port.
/// The caller authenticates the retained receipt, input event and companion.
/// # Errors
/// Refuses missing evidence, changed pins/bindings/proposals and private refusals.
pub async fn verify(
    loader: &impl ExecutionLoader,
    delivery: &MessageEnvelope,
    original: &CommittedOutcome,
    selection: Selection,
    retained: Retained<'_>,
    renderer: &str,
    momentum: &str,
) -> Result<ValidatedExplanation, ReplayError> {
    verify_execution(loader, delivery, original, selection, renderer, momentum).await?;
    let decision = decode_decision(original.decision().ok_or(ReplayError::Binding)?)
        .map_err(ReplayError::Publication)?;
    validate(original.record(), &decision, retained)
}
fn validate(
    record: &ResearchRecord,
    decision: &DecisionProvenance,
    retained: Retained<'_>,
) -> Result<ValidatedExplanation, ReplayError> {
    let companion = Companion::decode(retained.companion).map_err(|_| ReplayError::Binding)?;
    companion
        .bind(record, decision)
        .map_err(|_| ReplayError::Binding)?;
    let prepared =
        Prepared::new(record, decision, retained.profile).map_err(|_| ReplayError::Revision)?;
    let expected = prepared.binding();
    if companion.binding.anchor != expected.anchor
        || companion.binding.profile != expected.profile
        || companion.binding.evidence != expected.evidence
    {
        return Err(ReplayError::Binding);
    }
    let (rendered, outcome) = match companion.outcome {
        Outcome::Generated { proposal } => {
            let allowlist = crate::explanation::Allowlist::from_verified(record, decision)
                .map_err(|_| ReplayError::Binding)?;
            let rendered = allowlist
                .validate(
                    &proposal
                        .canonical_bytes()
                        .map_err(|_| ReplayError::Binding)?,
                )
                .map_err(|_| ReplayError::Mismatch)?;
            let outcome = Outcome::Generated {
                proposal: rendered.proposal().clone(),
            };
            (rendered, outcome)
        }
        Outcome::Fallback { reason, .. } => {
            let rendered = prepared
                .fallback(reason)
                .map_err(|_| ReplayError::Mismatch)?
                .explanation;
            let outcome = Outcome::Fallback {
                reason,
                proposal: rendered.proposal().clone(),
            };
            (rendered, outcome)
        }
        Outcome::Refused { .. } => return Err(ReplayError::Mismatch),
    };
    let rebuilt = Companion {
        binding: expected.clone(),
        outcome,
    };
    if rebuilt
        .canonical_bytes()
        .map_err(|_| ReplayError::Mismatch)?
        != retained.companion
    {
        return Err(ReplayError::Mismatch);
    }
    Ok(rendered)
}
