//! Owner lifecycle command validation and exact publication; authority is an outer obligation.
use crate::publication::{PublicationError as Error, PublicationErrorKind as Kind};
use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageEnvelope, MessageMetadata,
    research::{LifecycleChange, ResearchArtifact, ResearchLifecycle, ResearchRecord},
};
use edgeagent_research_domain::lifecycle::{Head, State};
/// Owner lifecycle event, sharing the scoped Research partition.
pub const CHANGED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.lifecycle-changed.v1",
    "urn:edgeagent:schema:research:record:v1",
    Component::Research,
    "research-request",
    EventRetention::Workflow,
);
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
/// Map the wire transition into independent domain policy.
#[must_use]
pub const fn state(change: &LifecycleChange) -> State {
    match change {
        LifecycleChange::Invalidated => State::Invalidated,
        LifecycleChange::Expired => State::Expired,
        LifecycleChange::Withdrawn => State::Withdrawn,
        LifecycleChange::Corrected { .. } => State::Corrected,
    }
}
/// Validate an entire bounded consecutive history against immutable original bytes.
/// # Errors
/// Rejects wrong binding, illegal transitions, gaps or time reversal.
pub fn validate(base: &ResearchArtifact, history: &[ResearchLifecycle]) -> Result<Head, Error> {
    let digest = ResearchRecord::Artifact(base.clone())
        .digest()
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    if history.len() > 2 {
        return Err(invalid());
    }
    let mut head = Head {
        state: State::Published,
        revision: 0,
        effective_at_ms: base.request.decision_at_ms,
    };
    for event in history {
        ResearchRecord::Lifecycle(event.clone())
            .canonical_bytes()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        if event.scope != base.request.scope
            || event.request_id != base.request.request_id
            || event.artifact != digest
        {
            return Err(invalid());
        }
        head = head
            .transition(
                event.revision,
                event.effective_at_ms,
                base.expires_at_ms,
                state(&event.change),
            )
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    }
    Ok(head)
}
/// Build a publication after owner-side history and correction-target checks.
/// # Errors
/// Rejects invalid contract/time or envelope construction.
pub fn publication(event: &ResearchLifecycle, time: &str) -> Result<MessageEnvelope, Error> {
    publication_with_cause(event, time, None)
}
/// Retain the exact owner decision event as causation for a configured invalidation.
pub fn publication_with_cause(
    event: &ResearchLifecycle,
    time: &str,
    cause: Option<&str>,
) -> Result<MessageEnvelope, Error> {
    let record = ResearchRecord::Lifecycle(event.clone());
    let digest = record
        .digest()
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    let partition = format!("research-request/{}/{}", event.scope, event.request_id);
    MessageEnvelope::from_payload(
        MessageMetadata {
            id: digest.clone(),
            source: Component::Research.source_uri().into(),
            message_type: CHANGED.message_type().into(),
            subject: partition.clone(),
            time: time.into(),
            data_schema: CHANGED.data_schema().into(),
            correlation_id: event.artifact.clone(),
            causation_id: cause.map_or_else(|| format!("owner-{digest}"), str::to_owned),
            idempotency_key: digest.clone(),
            partition_key: partition,
            trace_parent: format!("00-1{}-1{}-01", &digest[..31], &digest[32..47]),
            trace_state: None,
        },
        &record,
    )
    .map_err(|e| Error::with_source(Kind::Invalid, e))
}
/// Validate exact owner source/route/record identity. Authentication remains external.
/// # Errors
/// Rejects malformed, foreign or mismatched envelopes before consumption.
pub fn decode(envelope: &MessageEnvelope) -> Result<ResearchLifecycle, Error> {
    CHANGED
        .validate_envelope(envelope)
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    let record: ResearchRecord = envelope
        .payload()
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    let digest = record
        .digest()
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    let ResearchRecord::Lifecycle(event) = record else {
        return Err(invalid());
    };
    let partition = format!("research-request/{}/{}", event.scope, event.request_id);
    if envelope.source() != Component::Research.source_uri()
        || envelope.id() != digest
        || envelope.subject() != Some(partition.as_str())
        || envelope.partition_key() != Some(partition.as_str())
        || envelope.extension("idempotencykey") != Some(digest.as_str())
        || envelope.extension("correlationid") != Some(event.artifact.as_str())
    {
        return Err(invalid());
    }
    Ok(event)
}

/// Evaluate a configured check over an intact original and optional retained validated decision.
/// Caller authenticates retention and selected configuration; this pure function grants no authority.
pub fn configured_change(
    base: &ResearchArtifact,
    effective_at_ms: i64,
    reassessment: Option<&edgeagent_contracts::research_decision::DecisionProvenance>,
) -> Result<Option<LifecycleChange>, Error> {
    if effective_at_ms < base.request.decision_at_ms {
        return Err(invalid());
    }
    if effective_at_ms >= base.expires_at_ms {
        return Ok(Some(LifecycleChange::Expired));
    }
    let next = reassessment.ok_or_else(invalid)?;
    next.canonical_bytes()
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    let a = &base.request;
    let b = &next.request;
    if a.scope != b.scope
        || a.implementation != b.implementation
        || a.configuration != b.configuration
        || a.instruments != b.instruments
        || a.lookback_sessions != b.lookback_sessions
        || a.request_id == b.request_id
        || b.decision_at_ms <= a.decision_at_ms
        || b.decision_at_ms > effective_at_ms
    {
        return Err(invalid());
    }
    let edgeagent_contracts::research_decision::Assessment::Evaluated { eligible, .. } =
        &next.assessment
    else {
        return Err(invalid());
    };
    Ok(base
        .ranking
        .iter()
        .any(|c| !eligible.contains(&c.instrument))
        .then_some(LifecycleChange::Invalidated))
}
