//! Receipt-first orchestration; owner I/O occurs before the completion transaction.
use crate::{
    abstain, evaluate,
    publication::{
        self, CommittedOutcome, PreparedCompletion, PublicationError as Error,
        PublicationErrorKind as Kind, ResearchPublication,
    },
};
use edgeagent_contracts::{
    MessageEnvelope, research::AbstentionReason, research_acquisition::ResearchEvidence,
};
use edgeagent_research_domain::{EvidencePolicy, FrozenSeries};
/// Authenticated owner inputs under one explicitly selected immutable research policy.
pub struct LoadedEvidence {
    /// Owner/profile-selected scope, sessions and revisions.
    pub policy: EvidencePolicy,
    /// Verified retained bytes converted into pure domain assertions.
    pub series: Vec<FrozenSeries>,
    /// Explicit expiry no later than retained evidence authority.
    pub expires_at_ms: i64,
}
/// Safe read classification; unavailable reads must not become evidence-policy abstentions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadError {
    /// Current scope, rights or retention denied.
    Denied,
    /// Corrupt bytes/descriptor/lineage.
    Invalid,
    /// Operator policy does not support this request.
    Unsupported,
    /// Transient owner/storage/process unavailability; retry without completing.
    Unavailable,
}
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("research evidence read refused")
    }
}
impl std::error::Error for LoadError {}
/// Consumer-owned substitution boundary; the concrete adapter authenticates owner reads.
pub trait EvidenceLoader: Sync {
    /// Load only exact authorized evidence, never arbitrary client locations.
    fn load(
        &self,
        delivery: &MessageEnvelope,
    ) -> impl std::future::Future<Output = Result<LoadedEvidence, LoadError>> + Send;
}
/// Recover before evidence reads; evaluate and atomically retain exactly one outcome.
/// # Errors
/// Invalid ingress/policy, conflicting intent or unavailable storage/read rejects completion.
pub async fn process(
    store: &impl ResearchPublication,
    loader: &impl EvidenceLoader,
    delivery: &MessageEnvelope,
    time: &str,
) -> Result<CommittedOutcome, Error> {
    let request = publication::evaluation_request(delivery)?;
    if let Some(receipt) = store.recover(delivery).await? {
        return Ok(receipt);
    }
    let outcome = if delivery.message_type() == publication::EVALUATE.message_type() {
        // Legacy ingress has no owner-authorized evidence descriptor; drain safely.
        abstain(request, AbstentionReason::MissingEntitlement)
    } else {
        match ResearchEvidence::from_envelope(delivery).map_err(|_| Error::new(Kind::Invalid))? {
            ResearchEvidence::Abstained { reason, .. } => abstain(request, reason),
            ResearchEvidence::Ready { .. } => match loader.load(delivery).await {
                Ok(loaded) => evaluate(
                    request,
                    &loaded.policy,
                    &loaded.series,
                    loaded.expires_at_ms,
                ),
                Err(LoadError::Unavailable) => return Err(Error::new(Kind::Storage)),
                Err(error) => abstain(
                    request,
                    match error {
                        LoadError::Denied => AbstentionReason::MissingEntitlement,
                        LoadError::Unsupported => AbstentionReason::UnsupportedUniverse,
                        _ => AbstentionReason::InvalidEvidence,
                    },
                ),
            },
        }
    }
    .map_err(|_| Error::new(Kind::Invalid))?;
    store
        .commit(&PreparedCompletion::new(delivery.clone(), &outcome, time)?)
        .await
}
