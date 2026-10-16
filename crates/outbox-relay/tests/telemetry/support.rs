use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use edgeagent_contracts::{Component, EventRetention, MessageDefinition, MessageEnvelope};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, LeaseGeneration, MessagePublisher,
    OutboxRelayStore, OutboxRetryDelay, OutboxStoreError, OutboxStoreErrorKind, OutboxStoreFuture,
    PublishDisposition, PublishError, PublishErrorKind, PublishFuture, PublishReceipt,
};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

pub(super) const DEFINITION: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.request-received.v1",
    "urn:edgeagent:schema:research-request-received:v1",
    Component::Gateway,
    "research-request",
    EventRetention::Workflow,
);
pub(super) const PRIVATE: &str = "private-rejected-record-174";
pub(super) const OWNER: &str = "relay_telemetry";
const GENERATION: u64 = 73;

#[derive(Debug, Default)]
pub(super) struct Fields(pub(super) BTreeMap<String, String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.to_string());
    }
}

pub(super) struct Capture(pub(super) Arc<Mutex<Vec<Fields>>>);

impl Subscriber for Capture {
    fn register_callsite(
        &self,
        _metadata: &'static Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "edgeagent.event_spine"
    }
    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _span: &Id, _values: &Record<'_>) {}
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
    fn enter(&self, _span: &Id) {}
    fn exit(&self, _span: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(fields);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Operation {
    Claim,
    Publish,
    Mark,
    Retry,
    Quarantine,
}

pub(super) type Calls = Arc<Mutex<Vec<Operation>>>;

fn record(calls: &Calls, operation: Operation) {
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(operation);
}

pub(super) struct Store {
    pub(super) claim: Option<ClaimedMessage>,
    pub(super) original: ClaimedMessage,
    pub(super) calls: Calls,
    pub(super) fault: Option<(Operation, OutboxStoreErrorKind)>,
    pub(super) code: Option<FailureCode>,
}

impl Store {
    fn check(&self, operation: Operation) -> Result<(), OutboxStoreError> {
        if let Some((failed, kind)) = self.fault
            && failed == operation
        {
            return Err(OutboxStoreError::with_source(
                kind,
                std::io::Error::other(PRIVATE),
            ));
        }
        Ok(())
    }

    fn check_claim(&self, claim: &ClaimedMessage, owner: &str) {
        assert_eq!(owner, OWNER);
        assert_eq!(claim, &self.original);
        assert_eq!(claim.lease_generation().get(), GENERATION);
    }
}

impl OutboxRelayStore for Store {
    fn claim_one<'a>(
        &'a mut self,
        owner: &'a str,
        lease: LeaseDuration,
    ) -> OutboxStoreFuture<'a, Option<ClaimedMessage>> {
        record(&self.calls, Operation::Claim);
        assert_eq!(owner, OWNER);
        assert_eq!(lease.get(), Duration::from_secs(30));
        Box::pin(async move {
            self.check(Operation::Claim)?;
            Ok(self.claim.take())
        })
    }
    fn mark_published<'a>(
        &'a mut self,
        claim: &'a ClaimedMessage,
        owner: &'a str,
    ) -> OutboxStoreFuture<'a, ()> {
        record(&self.calls, Operation::Mark);
        self.check_claim(claim, owner);
        Box::pin(async move { self.check(Operation::Mark) })
    }
    fn release_for_retry<'a>(
        &'a mut self,
        claim: &'a ClaimedMessage,
        owner: &'a str,
        delay: OutboxRetryDelay,
        code: FailureCode,
    ) -> OutboxStoreFuture<'a, ()> {
        record(&self.calls, Operation::Retry);
        self.check_claim(claim, owner);
        assert!((Duration::from_millis(500)..=Duration::from_secs(1)).contains(&delay.get()));
        self.code = Some(code);
        Box::pin(async move { self.check(Operation::Retry) })
    }
    fn quarantine<'a>(
        &'a mut self,
        claim: &'a ClaimedMessage,
        owner: &'a str,
        code: FailureCode,
    ) -> OutboxStoreFuture<'a, ()> {
        record(&self.calls, Operation::Quarantine);
        self.check_claim(claim, owner);
        self.code = Some(code);
        Box::pin(async move { self.check(Operation::Quarantine) })
    }
}

pub(super) struct Publisher {
    pub(super) result: Result<PublishDisposition, PublishErrorKind>,
    pub(super) calls: Calls,
    pub(super) original: ClaimedMessage,
}

impl MessagePublisher for Publisher {
    fn publish<'a>(
        &'a self,
        definition: MessageDefinition,
        envelope: &'a MessageEnvelope,
    ) -> PublishFuture<'a> {
        record(&self.calls, Operation::Publish);
        assert_eq!(definition, DEFINITION);
        assert_eq!(
            envelope.to_json().ok().as_deref(),
            Some(self.original.envelope_bytes())
        );
        Box::pin(async move {
            self.result
                .map(PublishReceipt::new)
                .map_err(|kind| PublishError::with_source(kind, std::io::Error::other(PRIVATE)))
        })
    }
}

pub(super) fn claim(
    attempt: u32,
    rejected: Option<&str>,
) -> Result<ClaimedMessage, Box<dyn Error>> {
    let mut wire: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../contracts/fixtures/v1/research-request-received.json"
    ))?;
    wire["data"] = serde_json::json!({"private_payload": PRIVATE});
    let envelope = MessageEnvelope::from_json(&serde_json::to_vec(&wire)?)?;
    let stored = |field, value: &str| {
        if rejected == Some(field) {
            PRIVATE.to_owned()
        } else {
            value.to_owned()
        }
    };
    Ok(ClaimedMessage::new(
        stored("source", envelope.source()),
        stored("id", envelope.id()),
        stored("type", envelope.message_type()),
        DEFINITION.subject()?,
        if rejected == Some("bytes") {
            PRIVATE.as_bytes().to_vec()
        } else {
            envelope.to_json()?
        },
        attempt,
        LeaseGeneration::new(GENERATION)?,
    )?)
}
