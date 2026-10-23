use super::*;
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata};
use std::future::Future;

/// Exact reference-only terminal event. Construction validates bytes, not database commit.
#[derive(Clone)]
pub struct TerminalReceipt {
    envelope: MessageEnvelope,
    decision: EvidenceDecision,
    bytes: Vec<u8>,
}
impl fmt::Debug for TerminalReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalReceipt").finish_non_exhaustive()
    }
}
impl TerminalReceipt {
    /// Build the first candidate from trusted commit time and the causing command.
    /// Identity is stable per acquisition; retries must recover the retained bytes.
    /// # Errors
    /// Rejects invalid routes, command bindings, RFC3339 time or correlation context.
    pub fn new(
        delivery: &MessageEnvelope,
        decision: &EvidenceDecision,
        time: &str,
    ) -> Result<Self, Error> {
        let command =
            AcquireEvidence::from_envelope(delivery).map_err(|_| Error::new(K::Invalid))?;
        if command != decision.data().request {
            return Err(Error::new(K::Conflict));
        }
        let definition = DEFINITIONS[1];
        let required = |key| {
            delivery
                .extension(key)
                .map(str::to_owned)
                .ok_or_else(|| Error::new(K::Invalid))
        };
        let envelope = MessageEnvelope::from_payload(
            MessageMetadata {
                id: format!("evidence-decision-{}", command.request.as_str()),
                source: Component::MarketData.source_uri().into(),
                message_type: definition.message_type().into(),
                subject: format!("acquisition/{}", command.request.as_str()),
                time: time.into(),
                data_schema: definition.data_schema().into(),
                correlation_id: required("correlationid")?,
                causation_id: delivery.id().into(),
                idempotency_key: format!("evidence-decision-{}", command.request.as_str()),
                partition_key: format!("acquisition/{}", command.request.as_str()),
                trace_parent: required("traceparent")?,
                trace_state: delivery.extension("tracestate").map(str::to_owned),
            },
            decision,
        )
        .map_err(|_| Error::new(K::Invalid))?;
        let bytes = envelope.to_json().map_err(|_| Error::new(K::Invalid))?;
        Self::decode(&bytes, &command)
    }
    /// Bound/decode exact retained event bytes and verify their stable command/event identity.
    /// A receipt is historical evidence, never current authority to access an object.
    /// # Errors
    /// Rejects malformed bytes, wrong route, changed command binding or event identity.
    pub fn decode(bytes: &[u8], command: &AcquireEvidence) -> Result<Self, Error> {
        let envelope = MessageEnvelope::from_json(bytes).map_err(|_| Error::new(K::Invalid))?;
        let decision =
            EvidenceDecision::from_envelope(&envelope).map_err(|_| Error::new(K::Invalid))?;
        if &decision.data().request != command
            || envelope.id() != format!("evidence-decision-{}", command.request.as_str())
            || envelope.to_json().map_err(|_| Error::new(K::Invalid))? != bytes
        {
            return Err(Error::new(K::Invalid));
        }
        Ok(Self {
            envelope,
            decision,
            bytes: bytes.to_vec(),
        })
    }
    /// Exact immutable bytes, including original causation/correlation and event time.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Validated reference-only terminal decision.
    #[must_use]
    pub const fn decision(&self) -> &EvidenceDecision {
        &self.decision
    }
    /// Validated retained event for the existing outbox enqueue path.
    #[must_use]
    pub const fn envelope(&self) -> &MessageEnvelope {
        &self.envelope
    }
}
/// Application-owned atomic terminal boundary; implementations acknowledge no broker message.
pub trait AcquisitionPublication: Send + Sync {
    /// If already terminal, atomically record this delivery and return the exact receipt.
    /// Otherwise return None without committing an inbox marker or executing acquisition.
    fn recover(
        &self,
        delivery: &MessageEnvelope,
    ) -> impl Future<Output = Result<Option<TerminalReceipt>, Error>> + Send;
    /// Commit receipt, inbox and outbox together. Ready requires fresh local authority.
    /// `access.authorize` must use owner-retained policy, not perform external I/O in the transaction.
    /// Cancellation/failed confirmation is uncertain: recover by command, never compensate.
    fn commit(
        &self,
        delivery: &MessageEnvelope,
        proposal: &TerminalProposal,
        access: &impl PublicationAuthority,
    ) -> impl Future<Output = Result<TerminalReceipt, Error>> + Send;
}
/// One bounded delivery: recover before work, prepare outside transactions, then commit.
/// Only a confirmed result permits broker ACK. Cancellation may leave stored objects or a
/// committed receipt; re-enter with the same command. This is not a worker/intake loop.
/// # Errors
/// Preserves preparation/publication failures; never ACKs or retries on its own.
pub async fn finish_acquisition<F: Future<Output = Result<TerminalProposal, Error>>>(
    store: &impl AcquisitionPublication,
    delivery: &MessageEnvelope,
    access: &impl PublicationAuthority,
    prepare: impl FnOnce() -> F,
) -> Result<TerminalReceipt, Error> {
    if let Some(receipt) = store.recover(delivery).await? {
        return Ok(receipt);
    }
    let proposal = prepare().await?;
    store.commit(delivery, &proposal, access).await
}
