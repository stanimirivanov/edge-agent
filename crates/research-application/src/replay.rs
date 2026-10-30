//! Recalculate retained results without a publication/store capability.
use crate::{
    EvaluationError, RENDERER_REVISION, RenderedOutcome, abstain, evaluate,
    publication::{self, CommittedOutcome, PublicationError},
    worker::{EvidenceLoader, LoadError},
};
use edgeagent_contracts::{
    MessageEnvelope,
    research::{AbstentionReason, ResearchRecord},
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
    if renderer != RENDERER_REVISION || momentum != edgeagent_research_domain::momentum::REVISION {
        return Err(ReplayError::Revision);
    }
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
