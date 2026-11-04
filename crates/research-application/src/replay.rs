//! Recalculate retained results without a publication/store capability.
use crate::{
    EvaluationError, RENDERER_REVISION, RenderedOutcome, abstain, evaluate,
    publication::{self, CommittedOutcome, PublicationError},
    worker::{EvidenceLoader, ExecutionLoader, LoadError},
};
use edgeagent_contracts::{
    MessageEnvelope,
    research::{AbstentionReason, ResearchRecord, ResearchRequest},
    research_acquisition::ResearchEvidence,
};
/// Safe replay refusal; read failures are never successful evidence abstentions.
#[derive(Debug)]
pub enum ReplayError {
    /// Supplied code revision is not this compiled implementation.
    Revision,
    /// Retained receipt/input identities differ.
    Binding,
    /// Original command validation failed.
    Publication(PublicationError),
    /// Current evidence could not be authorized/loaded; no replay result exists.
    Read(LoadError),
    /// Calculation or bound policy validation failed.
    Evaluation(EvaluationError),
    /// Recalculated bytes differ from the retained immutable original.
    Mismatch,
}
impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("research replay refused")
    }
}
impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Publication(e) => Some(e),
            Self::Read(e) => Some(e),
            Self::Evaluation(e) => Some(e),
            _ => None,
        }
    }
}
/// Verify original binding and pinned code names, then recalculate and compare exact bytes.
/// The caller authenticates the retained receipt and original input independently.
/// No completion, inbox, outbox or broker port is accepted here.
/// # Errors
/// Refuses changed revisions/intent, unavailable evidence, invalid policy or unequal output.
pub async fn verify(
    loader: &impl EvidenceLoader,
    delivery: &MessageEnvelope,
    receipt: &CommittedOutcome,
    renderer: &str,
    momentum: &str,
) -> Result<RenderedOutcome, ReplayError> {
    check_revisions(renderer, momentum)?;
    if receipt.decision().is_some() {
        return Err(ReplayError::Binding);
    }
    let request = bound_request(delivery, receipt)?;
    let result = if delivery.message_type() == publication::EVALUATE.message_type() {
        abstain(request, AbstentionReason::MissingEntitlement)
    } else {
        match ResearchEvidence::from_envelope(delivery).map_err(|e| {
            ReplayError::Publication(PublicationError::with_source(
                publication::PublicationErrorKind::Invalid,
                e,
            ))
        })? {
            ResearchEvidence::Abstained { reason, .. } => abstain(request, reason),
            ResearchEvidence::Ready { .. } => {
                // Never recover-and-return the receipt, or convert a current read failure
                // into apparent reproduction of a historical read-failure abstention.
                let loaded = loader.load(delivery).await.map_err(ReplayError::Read)?;
                evaluate(
                    request,
                    &loaded.policy,
                    &loaded.series,
                    loaded.expires_at_ms,
                )
            }
        }
    }
    .map_err(ReplayError::Evaluation)?;
    if result.bytes() != receipt.bytes() {
        return Err(ReplayError::Mismatch);
    }
    Ok(result)
}

fn check_revisions(renderer: &str, momentum: &str) -> Result<(), ReplayError> {
    if renderer != RENDERER_REVISION || momentum != edgeagent_research_domain::momentum::REVISION {
        return Err(ReplayError::Revision);
    }
    Ok(())
}

fn bound_request(
    delivery: &MessageEnvelope,
    receipt: &CommittedOutcome,
) -> Result<ResearchRequest, ReplayError> {
    let request = publication::evaluation_request(delivery).map_err(ReplayError::Publication)?;
    let retained = match receipt.record() {
        ResearchRecord::Artifact(a) => &a.request,
        ResearchRecord::Abstention(a) => &a.request,
        _ => return Err(ReplayError::Binding),
    };
    if retained != &request
        || receipt.publication().extension("causationid") != Some(delivery.id())
        || receipt.publication().extension("correlationid") != delivery.extension("correlationid")
    {
        return Err(ReplayError::Binding);
    }
    Ok(request)
}

/// Reproduce exact terminal and decision bytes using the authenticated retained selection.
/// The caller must obtain the selection and receipt from the same historical snapshot.
/// # Errors
/// Refuses incompatible code, evidence read failures and any terminal or policy drift.
pub async fn verify_execution(
    loader: &impl ExecutionLoader,
    delivery: &MessageEnvelope,
    receipt: &CommittedOutcome,
    selection: crate::execution::Selection,
    renderer: &str,
    momentum: &str,
) -> Result<Vec<u8>, ReplayError> {
    check_revisions(renderer, momentum)?;
    let request = bound_request(delivery, receipt)?;
    let retained = publication::decode_decision(receipt.decision().ok_or(ReplayError::Binding)?)
        .map_err(ReplayError::Publication)?;
    selection
        .verify_provenance(&retained)
        .map_err(ReplayError::Publication)?;

    let execution = if delivery.message_type() == publication::EVALUATE.message_type() {
        selection.refused(request, LoadError::Denied)
    } else {
        match ResearchEvidence::from_envelope(delivery).map_err(|e| {
            ReplayError::Publication(PublicationError::with_source(
                publication::PublicationErrorKind::Invalid,
                e,
            ))
        })? {
            ResearchEvidence::Abstained { reason, .. } => selection.negative(request, reason),
            ResearchEvidence::Ready { .. } => {
                let input = loader
                    .load_execution(delivery)
                    .await
                    .map_err(ReplayError::Read)?;
                selection.validate(
                    request,
                    &input.loaded.policy,
                    &input.loaded.series,
                    input.loaded.expires_at_ms,
                    crate::governed::ContextInputs {
                        constraints: crate::governed::ConstraintInputs {
                            volumes: &input.volumes,
                            opens: &input.opens,
                        },
                        context: &input.context,
                    },
                )
            }
        }
    }
    .map_err(ReplayError::Publication)?;
    let provenance = execution.provenance().map_err(ReplayError::Publication)?;
    let original_decision = retained
        .canonical_bytes()
        .map_err(|_| ReplayError::Binding)?;
    let replayed_decision = provenance
        .canonical_bytes()
        .map_err(|_| ReplayError::Mismatch)?;
    if execution.outcome.bytes() != receipt.bytes() || replayed_decision != original_decision {
        return Err(ReplayError::Mismatch);
    }
    Ok(execution.outcome.bytes().to_vec())
}
