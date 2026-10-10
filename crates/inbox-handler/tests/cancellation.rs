//! Deterministic coordinator cancellation contracts, not adapter atomicity proofs.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_handler::{HandlerErrorKind, HandlerPolicy, HandlingOutcome, handle_once};
use edgeagent_messaging::{
    DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata, DeliverySettlement,
    DeliverySubject, FailureCode, InboundMessageStore, InboundProcessingError, InboundQuarantine,
    InboxDisposition, InboxFuture, InboxStoreError, InboxStoreErrorKind, MessageDelivery,
    QuarantineDisposition, SettlementFuture,
};
use serde_json::json;
use std::collections::{HashMap, hash_map::Entry};
use std::error::Error;
use std::future::{Future, pending};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const CONSUMER: &str = "execution_simulator_v1";
const POISON: FailureCode = FailureCode::from_static("envelope_invalid");

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Pause {
    #[default]
    None,
    BeforeCommit,
    AfterCommit,
}

#[derive(Clone, Eq, PartialEq)]
struct RetainedEvidence {
    subject: String,
    payload: Vec<u8>,
    failure_code: String,
    first_attempt: u32,
    last_attempt: u32,
}

impl RetainedEvidence {
    fn matches(&self, evidence: InboundQuarantine<'_>) -> bool {
        self.subject == evidence.transport_subject()
            && self.payload == evidence.payload()
            && self.failure_code == evidence.failure_code()
    }
}

/// A synchronous model of durable state with explicit commit/confirmation gaps.
/// Its commit model makes test ordering observable; real adapters must prove
/// their own atomicity, rollback and durability separately.
#[derive(Default)]
struct Store {
    process_pause: Pause,
    quarantine_pause: Pause,
    inbox: HashMap<(String, String, String), Vec<u8>>,
    evidence: HashMap<(String, String), RetainedEvidence>,
    domain_transitions: usize,
    process_calls: usize,
    quarantine_calls: usize,
}

impl InboundMessageStore for Store {
    fn process<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        _registry: &'operation MessageRegistry<'_>,
        envelope: &'operation MessageEnvelope,
    ) -> InboxFuture<'operation, Result<InboxDisposition, InboundProcessingError>> {
        // Count invocation even when a caller constructs but never polls the
        // operation: cancellation must not start replacement work either.
        self.process_calls += 1;
        Box::pin(async move {
            if self.process_pause == Pause::BeforeCommit {
                pending::<()>().await;
            }
            let bytes = envelope.to_json().map_err(|error| {
                InboundProcessingError::Store(InboxStoreError::with_source(
                    InboxStoreErrorKind::Contract,
                    error,
                ))
            })?;
            let key = (
                consumer_name.to_owned(),
                envelope.source().to_owned(),
                envelope.id().to_owned(),
            );
            let disposition = match self.inbox.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(bytes);
                    self.domain_transitions += 1;
                    InboxDisposition::Applied
                }
                Entry::Occupied(entry) if entry.get() == &bytes => InboxDisposition::Duplicate,
                Entry::Occupied(_) => {
                    return Err(InboundProcessingError::Store(InboxStoreError::new(
                        InboxStoreErrorKind::MessageIdentityConflict,
                    )));
                }
            };
            if self.process_pause == Pause::AfterCommit {
                pending::<()>().await;
            }
            Ok(disposition)
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        evidence: InboundQuarantine<'operation>,
    ) -> InboxFuture<'operation, Result<QuarantineDisposition, InboxStoreError>> {
        self.quarantine_calls += 1;
        Box::pin(async move {
            if self.quarantine_pause == Pause::BeforeCommit {
                pending::<()>().await;
            }
            let key = (consumer_name.to_owned(), evidence.delivery_key().to_owned());
            let disposition = match self.evidence.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(RetainedEvidence {
                        subject: evidence.transport_subject().to_owned(),
                        payload: evidence.payload().to_vec(),
                        failure_code: evidence.failure_code().to_owned(),
                        first_attempt: evidence.delivery_attempt(),
                        last_attempt: evidence.delivery_attempt(),
                    });
                    QuarantineDisposition::Inserted
                }
                Entry::Occupied(mut entry) if entry.get().matches(evidence) => {
                    let retained = entry.get_mut();
                    retained.last_attempt = retained.last_attempt.max(evidence.delivery_attempt());
                    QuarantineDisposition::AlreadyPresent
                }
                Entry::Occupied(_) => {
                    return Err(InboxStoreError::new(InboxStoreErrorKind::Invariant));
                }
            };
            if self.quarantine_pause == Pause::AfterCommit {
                pending::<()>().await;
            }
            Ok(disposition)
        })
    }
}

#[derive(Default)]
struct Settlements {
    calls: AtomicUsize,
    acknowledged: AtomicUsize,
    quarantined: AtomicUsize,
    retried: AtomicUsize,
}

struct SettlementProbe(Arc<Settlements>);

impl DeliverySettlement for SettlementProbe {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        // Record construction, not only polling: either would be premature
        // before the store confirms commit, and Drop must not start one.
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        match disposition {
            DeliveryDisposition::Acknowledge => &self.0.acknowledged,
            DeliveryDisposition::Quarantined => &self.0.quarantined,
            DeliveryDisposition::RetryAfter(_) => &self.0.retried,
        }
        .fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

fn metadata(key: &str, subject: &str, attempt: u32) -> Result<DeliveryMetadata, Box<dyn Error>> {
    Ok(DeliveryMetadata::new(
        DeliveryMessageKey::new(key)?,
        DeliverySubject::new(subject)?,
        DeliveryAttempt::new(attempt)?,
    ))
}

fn delivery(
    payload: &[u8],
    attempt: u32,
    settlements: &Arc<Settlements>,
) -> Result<MessageDelivery, Box<dyn Error>> {
    Ok(MessageDelivery::new(
        payload.to_vec(),
        metadata("commands:41", &COMMAND.subject()?, attempt)?,
        Box::new(SettlementProbe(Arc::clone(settlements))),
    )?)
}

fn policy() -> Result<HandlerPolicy, Box<dyn Error>> {
    Ok(HandlerPolicy::new(
        CONSUMER,
        3,
        Duration::from_millis(1),
        Duration::from_millis(20),
    )?)
}

fn valid_payload() -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(COMMAND
        .build(
            MessageMetadata {
                id: "cancel-01".to_owned(),
                source: Component::Gateway.source_uri().to_owned(),
                message_type: COMMAND.message_type().to_owned(),
                subject: "order/cancel-01".to_owned(),
                time: "2026-10-10T00:00:00Z".to_owned(),
                data_schema: COMMAND.data_schema().to_owned(),
                correlation_id: "correlation-01".to_owned(),
                causation_id: "request-01".to_owned(),
                idempotency_key: "cancel-01".to_owned(),
                partition_key: "order/cancel-01".to_owned(),
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
                trace_state: None,
            },
            &json!({"mode": "dry_run"}),
        )?
        .to_json()?)
}

fn complete<F: Future>(future: F) -> Result<F::Output, Box<dyn Error>> {
    let mut future = Box::pin(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => Ok(output),
        Poll::Pending => Err(std::io::Error::other("test fake unexpectedly suspended").into()),
    }
}

#[test]
fn cancelled_processing_does_not_resolve_failure_or_settle_and_redelivery_applies_once()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = policy()?;
    let payload = valid_payload()?;
    for pause in [Pause::BeforeCommit, Pause::AfterCommit] {
        let mut store = Store {
            process_pause: pause,
            ..Store::default()
        };
        let settlements = Arc::new(Settlements::default());
        let mut future = Box::pin(handle_once(
            &mut store,
            &registry,
            &policy,
            delivery(&payload, 1, &settlements)?,
        ));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(future);
        let committed = usize::from(pause == Pause::AfterCommit);
        assert_eq!(store.inbox.len(), committed);
        assert_eq!(store.domain_transitions, committed);
        assert_eq!(store.process_calls, 1);
        assert_eq!(store.quarantine_calls, 0);
        assert_eq!(settlements.calls.load(Ordering::SeqCst), 0);

        store.process_pause = Pause::None;
        let outcome = complete(handle_once(
            &mut store,
            &registry,
            &policy,
            delivery(&payload, 2, &settlements)?,
        ))??;
        assert!(matches!(
            (pause, outcome),
            (Pause::BeforeCommit, HandlingOutcome::Applied { attempt: 2 })
                | (
                    Pause::AfterCommit,
                    HandlingOutcome::Duplicate { attempt: 2 }
                )
        ));
        assert_eq!(store.inbox.len(), 1);
        assert_eq!(store.domain_transitions, 1);
        assert_eq!(store.process_calls, 2);
        assert_eq!(store.quarantine_calls, 0);
        assert_eq!(settlements.calls.load(Ordering::SeqCst), 1);
        assert_eq!(settlements.acknowledged.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn cancelled_quarantine_does_not_settle_and_redelivery_discovers_exact_retained_evidence()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = policy()?;
    let payload = b"{private-invalid-payload";
    for pause in [Pause::BeforeCommit, Pause::AfterCommit] {
        let mut store = Store {
            quarantine_pause: pause,
            ..Store::default()
        };
        let settlements = Arc::new(Settlements::default());
        let mut future = Box::pin(handle_once(
            &mut store,
            &registry,
            &policy,
            delivery(payload, 1, &settlements)?,
        ));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(future);
        assert_eq!(
            store.evidence.len(),
            usize::from(pause == Pause::AfterCommit)
        );
        assert_eq!(settlements.calls.load(Ordering::SeqCst), 0);
        assert_eq!(store.process_calls, 0);
        assert_eq!(store.quarantine_calls, 1);

        store.quarantine_pause = Pause::None;
        let outcome = complete(handle_once(
            &mut store,
            &registry,
            &policy,
            delivery(payload, 2, &settlements)?,
        ))??;
        let expected = if pause == Pause::BeforeCommit {
            QuarantineDisposition::Inserted
        } else {
            QuarantineDisposition::AlreadyPresent
        };
        assert!(matches!(outcome, HandlingOutcome::Quarantined {
            attempt: 2, disposition, failure_code: "envelope_invalid", ..
        } if disposition == expected));
        assert_eq!(store.evidence.len(), 1);
        let retained = &store.evidence[&(CONSUMER.to_owned(), "commands:41".to_owned())];
        assert_eq!(retained.payload, payload);
        assert_eq!(retained.subject, COMMAND.subject()?);
        assert_eq!(retained.failure_code, POISON.as_str());
        assert_eq!(
            retained.first_attempt,
            if pause == Pause::BeforeCommit { 2 } else { 1 }
        );
        assert_eq!(retained.last_attempt, 2);
        assert_eq!(store.quarantine_calls, 2);
        assert_eq!(store.process_calls, 0);
        assert_eq!(store.domain_transitions, 0);
        assert_eq!(settlements.calls.load(Ordering::SeqCst), 1);
        assert_eq!(settlements.quarantined.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn quarantine_identity_requires_exact_evidence_and_observes_attempts_monotonically()
-> Result<(), Box<dyn Error>> {
    let mut store = Store::default();
    let initial = metadata("commands:41", "events.original", 3)?;
    let evidence = InboundQuarantine::new(&initial, b"original", POISON)?;
    assert_eq!(
        complete(store.quarantine(CONSUMER, evidence))??,
        QuarantineDisposition::Inserted
    );
    for attempt in [8, 2] {
        let observation = metadata("commands:41", "events.original", attempt)?;
        let evidence = InboundQuarantine::new(&observation, b"original", POISON)?;
        assert_eq!(
            complete(store.quarantine(CONSUMER, evidence))??,
            QuarantineDisposition::AlreadyPresent
        );
    }
    let key = (CONSUMER.to_owned(), "commands:41".to_owned());
    let retained = store.evidence[&key].clone();
    assert_eq!(retained.first_attempt, 3);
    assert_eq!(retained.last_attempt, 8);
    for (subject, payload, code) in [
        ("events.changed", b"original".as_slice(), POISON),
        ("events.original", b"changed".as_slice(), POISON),
        (
            "events.original",
            b"original".as_slice(),
            FailureCode::from_static("routing_invalid"),
        ),
    ] {
        let metadata = metadata("commands:41", subject, 9)?;
        let evidence = InboundQuarantine::new(&metadata, payload, code)?;
        assert_eq!(
            complete(store.quarantine(CONSUMER, evidence))?
                .err()
                .map(|error| error.kind()),
            Some(InboxStoreErrorKind::Invariant)
        );
        assert!(store.evidence[&key] == retained);
    }
    for (consumer, delivery_key) in [
        ("another_consumer", "commands:41"),
        (CONSUMER, "commands:42"),
    ] {
        let metadata = metadata(delivery_key, "events.independent", 1)?;
        let evidence = InboundQuarantine::new(&metadata, b"independent", POISON)?;
        assert_eq!(
            complete(store.quarantine(consumer, evidence))??,
            QuarantineDisposition::Inserted
        );
    }
    assert_eq!(store.evidence.len(), 3);
    assert!(store.evidence[&key] == retained);
    Ok(())
}

#[test]
fn conflicting_quarantine_evidence_fails_closed_without_broker_settlement()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = policy()?;
    let mut store = Store::default();
    let original = metadata("commands:41", &COMMAND.subject()?, 1)?;
    complete(store.quarantine(
        CONSUMER,
        InboundQuarantine::new(&original, b"{original", POISON)?,
    ))??;
    let retained = store.evidence.values().next().cloned();
    let settlements = Arc::new(Settlements::default());

    let result = complete(handle_once(
        &mut store,
        &registry,
        &policy,
        delivery(b"{conflicting", 2, &settlements)?,
    ))?;

    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Inbox)
    );
    assert_eq!(settlements.calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.process_calls, 0);
    assert_eq!(store.evidence.len(), 1);
    assert!(store.evidence.values().next().cloned() == retained);
    Ok(())
}
