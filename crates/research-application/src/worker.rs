//! Receipt-first orchestration; owner I/O occurs before the completion transaction.
use crate::publication::{
    self, CommittedOutcome, PreparedCompletion, PublicationError as Error,
    PublicationErrorKind as Kind, ResearchPublication,
};
use edgeagent_contracts::MessageEnvelope;
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
    /// Owner retained fewer sessions than the exact requested lookback.
    InsufficientHistory,
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
/// Compatibility receipt recovery only. Fresh execution must use process_execution.
/// # Errors
/// Invalid ingress, missing receipts, conflicting intent or storage failures refuse recovery.
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
    let _ = (request, loader, time);
    Err(Error::new(Kind::Invalid))
}
/// Complete authorized owner inputs for the exact configured execution.
pub struct ExecutionEvidence {
    /// Authorized base policy and close series.
    pub loaded: LoadedEvidence,
    /// Authorized complete selected volumes.
    pub volumes: Vec<edgeagent_research_domain::liquidity::Input>,
    /// Authorized complete selected opens/actions.
    pub opens: Vec<edgeagent_research_domain::market_constraints::Input>,
    /// Authorized owner context census.
    pub context: Vec<edgeagent_research_domain::context_policy::Input>,
}
/// Separate capability: a close-only loader cannot silently satisfy full M05 evidence.
pub trait ExecutionLoader: Sync {
    /// Load the complete companion through the owner boundary.
    fn load_execution(
        &self,
        delivery: &MessageEnvelope,
    ) -> impl std::future::Future<Output = Result<ExecutionEvidence, LoadError>> + Send;
}
/// Recover first, then read authorized inputs outside the transaction, reconcile, and commit.
pub async fn process_execution(
    store: &impl ResearchPublication,
    loader: &impl ExecutionLoader,
    selection: crate::execution::Selection,
    delivery: &MessageEnvelope,
    time: &str,
) -> Result<CommittedOutcome, Error> {
    let request = publication::evaluation_request(delivery)?;
    if let Some(receipt) = store.recover(delivery).await? {
        return Ok(receipt);
    }
    let outcome = if delivery.message_type() == publication::EVALUATE.message_type() {
        selection.refused(request, LoadError::Denied)?
    } else if let edgeagent_contracts::research_acquisition::ResearchEvidence::Abstained {
        reason,
        ..
    } =
        edgeagent_contracts::research_acquisition::ResearchEvidence::from_envelope(delivery)
            .map_err(|e| Error::with_source(Kind::Invalid, e))?
    {
        selection.negative(request, reason)?
    } else {
        match loader.load_execution(delivery).await {
            Ok(loaded) => selection.validate(
                request,
                &loaded.loaded.policy,
                &loaded.loaded.series,
                loaded.loaded.expires_at_ms,
                crate::governed::ContextInputs {
                    constraints: crate::governed::ConstraintInputs {
                        volumes: &loaded.volumes,
                        opens: &loaded.opens,
                    },
                    context: &loaded.context,
                },
            )?,
            Err(error) => selection.refused(request, error)?,
        }
    };
    store
        .commit(&PreparedCompletion::from_execution(
            delivery.clone(),
            &outcome,
            time,
        )?)
        .await
}
