//! Persistence-neutral research completion and exact-byte receipt boundary.
use crate::RenderedOutcome;
use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageEnvelope, MessageMetadata,
    MessageRegistry,
    research::{ResearchRecord, ResearchRequest},
};
/// Research-owned evaluation command; no topology or worker is activated here.
pub const EVALUATE: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.research.evaluate-request.v1",
    "urn:edgeagent:schema:research:record:v1",
    Component::Research,
    "research-request",
);
/// Research-owned immutable artifact notification.
pub const PUBLISHED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.artifact-published.v1",
    "urn:edgeagent:schema:research:record:v1",
    Component::Research,
    "research-request",
    EventRetention::Workflow,
);
/// Research-owned terminal abstention notification.
pub const ABSTAINED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.abstained.v1",
    "urn:edgeagent:schema:research:record:v1",
    Component::Research,
    "research-request",
    EventRetention::Workflow,
);
/// Companion provenance publication, independently replayable after the terminal outcome.
pub const DECISION: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.decision-recorded.v1",
    "urn:edgeagent:schema:research:decision:v1",
    Component::Research,
    "research-request",
    EventRetention::Workflow,
);
const DEFINITIONS: [MessageDefinition; 6] = [
    DECISION,
    EVALUATE,
    crate::lifecycle::CHANGED,
    PUBLISHED,
    ABSTAINED,
    edgeagent_contracts::research_acquisition::DECIDED,
];
/// Validated local definitions; caller provisions any future broker topology separately.
/// # Errors
/// Rejects an invalid compiled message definition.
pub fn registry() -> Result<MessageRegistry<'static>, PublicationError> {
    MessageRegistry::new(&DEFINITIONS)
        .map_err(|_| PublicationError::new(PublicationErrorKind::Invalid))
}
/// Payload-free persistence classification; ambiguous commits require retry by identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationErrorKind {
    /// Invalid binding, receipt or retained integrity.
    Invalid,
    /// Reused identity with different immutable bytes.
    Conflict,
    /// Driver failure; commit may have succeeded, so recover by the same request identity.
    Storage,
}
/// Safe diagnostic retaining a typed cause without formatting driver/payload details.
pub struct PublicationError {
    kind: PublicationErrorKind,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl PublicationError {
    /// Construct a safe classified rejection.
    #[must_use]
    pub const fn new(kind: PublicationErrorKind) -> Self {
        Self { kind, source: None }
    }
    /// Preserve an underlying cause without exposing it in diagnostics.
    #[must_use]
    pub fn with_source(
        kind: PublicationErrorKind,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }
    /// Stable caller-visible category.
    #[must_use]
    pub const fn kind(&self) -> PublicationErrorKind {
        self.kind
    }
}
impl std::fmt::Debug for PublicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for PublicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "research publication rejected: {:?}", self.kind)
    }
}
impl std::error::Error for PublicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|s| s as &(dyn std::error::Error + 'static))
    }
}
fn invalid() -> PublicationError {
    PublicationError::new(PublicationErrorKind::Invalid)
}
fn request(record: &ResearchRecord) -> Result<&ResearchRequest, PublicationError> {
    match record {
        ResearchRecord::Artifact(v) => Ok(&v.request),
        ResearchRecord::Abstention(v) => Ok(&v.request),
        _ => Err(invalid()),
    }
}
fn partition(r: &ResearchRequest) -> String {
    format!("research-request/{}/{}", r.scope, r.request_id)
}
fn definition(r: &ResearchRecord) -> Result<MessageDefinition, PublicationError> {
    match r {
        ResearchRecord::Artifact(_) => Ok(PUBLISHED),
        ResearchRecord::Abstention(_) => Ok(ABSTAINED),
        _ => Err(invalid()),
    }
}
/// Decode an authenticated evaluation delivery without evaluating evidence again.
/// # Errors
/// Rejects wrong producer/type, invalid request data or scoped identity mismatch.
pub fn evaluation_request(delivery: &MessageEnvelope) -> Result<ResearchRequest, PublicationError> {
    if delivery.message_type() == edgeagent_contracts::research_acquisition::DECIDED.message_type()
    {
        return edgeagent_contracts::research_acquisition::ResearchEvidence::from_envelope(
            delivery,
        )
        .map(|value| value.request().clone())
        .map_err(|_| invalid());
    }
    EVALUATE
        .validate_envelope(delivery)
        .map_err(|_| invalid())?;
    if ![
        Component::Gateway.source_uri(),
        Component::MarketData.source_uri(),
    ]
    .contains(&delivery.source())
    {
        return Err(invalid());
    }
    let record: ResearchRecord = delivery.payload().map_err(|_| invalid())?;
    record.canonical_bytes().map_err(|_| invalid())?;
    let key = record.digest().map_err(|_| invalid())?;
    let ResearchRecord::Request(request) = record else {
        return Err(invalid());
    };
    let partition = partition(&request);
    if delivery.partition_key() != Some(partition.as_str())
        || delivery.subject() != Some(partition.as_str())
        || delivery.extension("idempotencykey") != Some(key.as_str())
    {
        return Err(invalid());
    }
    Ok(request)
}
/// Validated candidate commit, prepared before opening a database transaction.
pub struct PreparedCompletion {
    delivery: MessageEnvelope,
    request_bytes: Vec<u8>,
    outcome_bytes: Vec<u8>,
    publication: MessageEnvelope,
    request: ResearchRequest,
    definition: MessageDefinition,
    execution: Option<Vec<u8>>,
    decision: Option<MessageEnvelope>,
}
impl PreparedCompletion {
    /// Prepare an independently reconciled terminal result for the original delivery.
    /// This low-level candidate retains ingress/identity checks. Fresh persistence requires
    /// from_execution, which also seals the complete configuration and provenance.
    /// # Errors
    /// Rejects mismatched delivery/request identity and existing publication contract failures.
    pub fn from_validated(
        delivery: MessageEnvelope,
        outcome: &crate::validation::ValidatedOutcome,
        time: &str,
    ) -> Result<Self, PublicationError> {
        Self::new(delivery, outcome.rendered(), time)
    }

    /// Bind the exact evaluation command, rendered outcome and explicit occurrence time.
    /// The outer adapter authenticates Gateway/MarketData and verifies evidence authority.
    /// Retained for compatibility/receipt recovery; fresh stores reject this legacy candidate.
    /// # Errors
    /// Rejects mismatched request content, identity/partition, producer or terminal payload.
    pub fn new(
        delivery: MessageEnvelope,
        outcome: &RenderedOutcome,
        time: &str,
    ) -> Result<Self, PublicationError> {
        let admitted = evaluation_request(&delivery)?;
        let req = request(outcome.record())?;
        if &admitted != req {
            return Err(invalid());
        }
        let req_record = ResearchRecord::Request(admitted);
        let request_bytes = req_record.canonical_bytes().map_err(|_| invalid())?;
        let key = req_record.digest().map_err(|_| invalid())?;
        let partition = partition(req);
        let def = definition(outcome.record())?;
        let publication = MessageEnvelope::from_payload(
            MessageMetadata {
                id: outcome.digest().into(),
                source: Component::Research.source_uri().into(),
                message_type: def.message_type().into(),
                subject: partition.clone(),
                time: time.into(),
                data_schema: def.data_schema().into(),
                correlation_id: delivery
                    .extension("correlationid")
                    .ok_or_else(invalid)?
                    .into(),
                causation_id: delivery.id().into(),
                idempotency_key: key,
                partition_key: partition,
                trace_parent: delivery
                    .extension("traceparent")
                    .ok_or_else(invalid)?
                    .into(),
                trace_state: delivery.extension("tracestate").map(str::to_owned),
            },
            outcome.record(),
        )
        .map_err(|_| invalid())?;
        CommittedOutcome::decode(
            outcome.bytes(),
            &publication.to_json().map_err(|_| invalid())?,
        )?;
        Ok(Self {
            delivery,
            request_bytes,
            outcome_bytes: outcome.bytes().to_vec(),
            publication,
            request: req.clone(),
            definition: def,
            execution: None,
            decision: None,
        })
    }
    /// Only sealed complete execution may prepare a fresh governed publication.
    /// ```compile_fail
    /// use edgeagent_contracts::MessageEnvelope;
    /// use edgeagent_research_application::{RenderedOutcome,publication::PreparedCompletion};
    /// fn bypass(delivery:MessageEnvelope,proposal:&RenderedOutcome) {
    ///     let _=PreparedCompletion::from_execution(delivery,proposal,"2026-10-31T00:00:00Z");
    /// }
    /// ```
    pub fn from_execution(
        delivery: MessageEnvelope,
        result: &crate::execution::ValidatedExecution,
        time: &str,
    ) -> Result<Self, PublicationError> {
        let mut prepared = Self::from_validated(delivery, &result.outcome, time)?;
        let provenance = result.provenance()?;
        provenance.bind(result.record()).map_err(|_| invalid())?;
        let mut metadata: MessageMetadata = MessageMetadata {
            id: provenance.digest().map_err(|_| invalid())?,
            source: Component::Research.source_uri().into(),
            message_type: DECISION.message_type().into(),
            subject: partition(prepared.request()),
            time: time.into(),
            data_schema: DECISION.data_schema().into(),
            correlation_id: prepared
                .delivery
                .extension("correlationid")
                .ok_or_else(invalid)?
                .into(),
            causation_id: prepared.publication.id().into(),
            idempotency_key: provenance.digest().map_err(|_| invalid())?,
            partition_key: partition(prepared.request()),
            trace_parent: prepared
                .delivery
                .extension("traceparent")
                .ok_or_else(invalid)?
                .into(),
            trace_state: prepared.delivery.extension("tracestate").map(str::to_owned),
        };
        // Identity covers the companion; causation binds the immutable terminal result.
        metadata.causation_id = provenance.outcome.clone();
        prepared.decision = Some(
            MessageEnvelope::from_payload(
                metadata,
                &edgeagent_contracts::research_decision::DecisionRecord::V1(provenance),
            )
            .map_err(|_| invalid())?,
        );
        prepared.execution = Some(result.selection.bytes().to_vec());
        Ok(prepared)
    }
    /// Exact complete execution bytes, absent on legacy preparations.
    pub fn execution_bytes(&self) -> Option<&[u8]> {
        self.execution.as_deref()
    }
    /// Immutable companion publication candidate.
    pub fn decision_publication(&self) -> Option<&MessageEnvelope> {
        self.decision.as_ref()
    }
    /// Exact authenticated command candidate; authentication itself is an outer obligation.
    #[must_use]
    pub const fn delivery(&self) -> &MessageEnvelope {
        &self.delivery
    }
    /// Exact canonical admitted request bytes used for scoped conflict checks.
    #[must_use]
    pub fn request_bytes(&self) -> &[u8] {
        &self.request_bytes
    }
    /// Exact terminal outcome bytes.
    #[must_use]
    pub fn outcome_bytes(&self) -> &[u8] {
        &self.outcome_bytes
    }
    /// Request bound by construction to this terminal outcome.
    #[must_use]
    pub const fn request(&self) -> &ResearchRequest {
        &self.request
    }
    /// Candidate publication definition.
    #[must_use]
    pub const fn definition(&self) -> MessageDefinition {
        self.definition
    }
    /// Exact candidate publication; retries may instead return an older retained outcome.
    #[must_use]
    pub const fn publication(&self) -> &MessageEnvelope {
        &self.publication
    }
}
/// Validated retained receipt; construction alone is not evidence that a transaction committed.
pub struct CommittedOutcome {
    record: ResearchRecord,
    bytes: Vec<u8>,
    publication: MessageEnvelope,
    decision: Option<MessageEnvelope>,
}
impl CommittedOutcome {
    /// Verify canonical terminal bytes, content/event identity and request binding.
    /// # Errors
    /// Rejects corrupt, mismatched, noncanonical or nonterminal retained records.
    pub fn decode(bytes: &[u8], publication: &[u8]) -> Result<Self, PublicationError> {
        let record = ResearchRecord::decode(bytes).map_err(|_| invalid())?;
        let req = request(&record)?;
        let req_digest = ResearchRecord::Request(req.clone())
            .digest()
            .map_err(|_| invalid())?;
        let envelope = MessageEnvelope::from_json(publication).map_err(|_| invalid())?;
        definition(&record)?
            .validate_envelope(&envelope)
            .map_err(|_| invalid())?;
        let payload: ResearchRecord = envelope.payload().map_err(|_| invalid())?;
        let partition = partition(req);
        if payload != record
            || envelope.id() != record.digest().map_err(|_| invalid())?
            || envelope.partition_key() != Some(partition.as_str())
            || envelope.subject() != Some(partition.as_str())
            || envelope.extension("idempotencykey") != Some(req_digest.as_str())
            || envelope.to_json().map_err(|_| invalid())? != publication
        {
            return Err(invalid());
        }
        Ok(Self {
            record,
            bytes: bytes.to_vec(),
            publication: envelope,
            decision: None,
        })
    }
    /// Attach an intact independently published companion to the historical receipt.
    pub fn with_decision(mut self, bytes: &[u8]) -> Result<Self, PublicationError> {
        let envelope = MessageEnvelope::from_json(bytes).map_err(|_| invalid())?;
        if envelope.to_json().map_err(|_| invalid())? != bytes {
            return Err(invalid());
        }
        let provenance = decode_decision(&envelope)?;
        provenance.bind(&self.record).map_err(|_| invalid())?;
        if envelope.extension("causationid") != Some(self.publication.id())
            || envelope.extension("correlationid") != self.publication.extension("correlationid")
        {
            return Err(invalid());
        }
        self.decision = Some(envelope);
        Ok(self)
    }
    /// Exact retained provenance event, absent for pre-upgrade receipts.
    pub fn decision(&self) -> Option<&MessageEnvelope> {
        self.decision.as_ref()
    }
    /// Exact original immutable outcome, including after retries/restarts.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Retained publication identity; do not substitute a newly computed event on retry.
    #[must_use]
    pub const fn publication(&self) -> &MessageEnvelope {
        &self.publication
    }
    /// Validated retained record.
    #[must_use]
    pub const fn record(&self) -> &ResearchRecord {
        &self.record
    }
}
/// Validate owner route and companion identity; caller authenticates the transport source.
pub fn decode_decision(
    envelope: &MessageEnvelope,
) -> Result<edgeagent_contracts::research_decision::DecisionProvenance, PublicationError> {
    DECISION
        .validate_envelope(envelope)
        .map_err(|_| invalid())?;
    let edgeagent_contracts::research_decision::DecisionRecord::V1(record): edgeagent_contracts::research_decision::DecisionRecord =
        envelope.payload().map_err(|_| invalid())?;
    record.canonical_bytes().map_err(|_| invalid())?;
    let partition = partition(&record.request);
    let hash = record.digest().map_err(|_| invalid())?;
    if envelope.id() != hash
        || envelope.extension("idempotencykey") != Some(hash.as_str())
        || envelope.subject() != Some(partition.as_str())
        || envelope.partition_key() != Some(partition.as_str())
        || envelope.extension("causationid") != Some(record.outcome.as_str())
    {
        return Err(invalid());
    }
    Ok(record)
}
/// Atomic research-owned completion/inbox/outbox port; implementation performs no broker I/O.
pub trait ResearchPublication: Sync {
    /// Recover retained completion before evidence reads or rendering.
    /// # Errors
    /// Rejects conflicting requests, corrupt retained integrity or storage failures.
    fn recover(
        &self,
        delivery: &MessageEnvelope,
    ) -> impl std::future::Future<Output = Result<Option<CommittedOutcome>, PublicationError>> + Send;
    /// Commit first outcome or recover the original bytes for the same scoped request.
    /// # Errors
    /// Conflicting request bytes reject; ambiguous storage failures must retry this identity.
    fn commit(
        &self,
        prepared: &PreparedCompletion,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, PublicationError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;
    fn artifact() -> std::result::Result<ResearchRecord, PublicationError> {
        ResearchRecord::decode(include_bytes!("../tests/fixtures/artifact-v1.json"))
            .map_err(|_| invalid())
    }
    fn metadata(
        record: &ResearchRecord,
        event: bool,
    ) -> std::result::Result<MessageMetadata, PublicationError> {
        let req = request(record)?;
        let def = if event { PUBLISHED } else { EVALUATE };
        Ok(MessageMetadata {
            id: if event {
                record.digest().map_err(|_| invalid())?
            } else {
                "delivery-01".into()
            },
            source: if event {
                Component::Research
            } else {
                Component::Gateway
            }
            .source_uri()
            .into(),
            message_type: def.message_type().into(),
            subject: partition(req),
            time: "2026-10-29T00:00:00Z".into(),
            data_schema: def.data_schema().into(),
            correlation_id: "correlation-01".into(),
            causation_id: "cause-01".into(),
            idempotency_key: ResearchRecord::Request(req.clone())
                .digest()
                .map_err(|_| invalid())?,
            partition_key: partition(req),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        })
    }
    #[test]
    fn evaluation_envelope_rejects_wrong_producer_partition_and_request_identity() -> Result {
        let artifact = artifact()?;
        let req = request(&artifact)?.clone();
        let payload = ResearchRecord::Request(req.clone());
        let good = MessageEnvelope::from_payload(metadata(&artifact, false)?, &payload)?;
        assert!(evaluation_request(&good)? == req);
        for field in 0..4 {
            let mut m = metadata(&artifact, false)?;
            match field {
                0 => m.source = Component::Research.source_uri().into(),
                1 => m.partition_key = "research-request/foreign/id".into(),
                2 => m.idempotency_key = "changed".into(),
                _ => m.message_type = PUBLISHED.message_type().into(),
            };
            assert!(evaluation_request(&MessageEnvelope::from_payload(m, &payload)?).is_err());
        }
        Ok(())
    }
    #[test]
    fn retained_terminal_receipt_rejects_mismatched_bytes_and_event_identity() -> Result {
        let record = artifact()?;
        let bytes = record.canonical_bytes()?;
        let envelope = MessageEnvelope::from_payload(metadata(&record, true)?, &record)?;
        assert_eq!(
            CommittedOutcome::decode(&bytes, &envelope.to_json()?)?.bytes(),
            bytes
        );
        for field in 0..3 {
            let mut m = metadata(&record, true)?;
            match field {
                0 => m.id = "changed".into(),
                1 => m.source = Component::Gateway.source_uri().into(),
                _ => m.idempotency_key = "changed".into(),
            };
            assert!(
                CommittedOutcome::decode(
                    &bytes,
                    &MessageEnvelope::from_payload(m, &record)?.to_json()?
                )
                .is_err()
            );
        }
        let mut changed = bytes;
        changed.push(b'\n');
        assert!(CommittedOutcome::decode(&changed, &envelope.to_json()?).is_err());
        Ok(())
    }
    #[test]
    fn publication_error_retains_cause_without_rendering_private_details() -> Result {
        use std::error::Error as _;
        let error = PublicationError::with_source(
            PublicationErrorKind::Storage,
            std::io::Error::other("private-test-sentinel"),
        );
        assert!(error.source().is_some());
        assert!(!format!("{error:?} {error}").contains("private-test-sentinel"));
        Ok(())
    }
}
