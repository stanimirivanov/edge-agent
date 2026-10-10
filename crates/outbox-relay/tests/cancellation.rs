//! Application-policy proofs with controlled commit/response boundaries.
//!
//! These durable in-memory fakes prove what `relay_once` does when cancelled,
//! not PostgreSQL transaction atomicity or a broker's deduplication window.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, LeaseGeneration, MessagePublisher,
    OutboxRelayStore, OutboxRetryDelay, OutboxStoreError, OutboxStoreErrorKind, OutboxStoreFuture,
    PublishDisposition, PublishError, PublishErrorKind, PublishFuture, PublishReceipt,
};
use edgeagent_outbox_relay::{RelayOutcome, RelayPolicy, relay_once};
use serde_json::json;
use std::error::Error;
use std::fmt::{Debug, Formatter};
use std::future::{Future, pending};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

const WORKER: &str = "cancellation_relay";
const LEASE: Duration = Duration::from_secs(30);
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Claim,
    Published,
    Retry,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Boundary {
    BeforeCommit,
    AfterCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pause(Operation, Boundary);

async fn pause_at(configured: Option<Pause>, operation: Operation, boundary: Boundary) {
    if configured == Some(Pause(operation, boundary)) {
        // A response can remain pending even after durable state changes.
        pending::<()>().await;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RowState {
    Available {
        at: Duration,
    },
    Leased {
        owner: String,
        generation: u64,
        expires_at: Duration,
    },
    Published,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Committed {
    Claim(u64),
    Published(u64),
    Retry(u64, Duration, FailureCode),
    Quarantine(u64, FailureCode),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    row: RowState,
    generation: u64,
    attempt: u32,
    journal: Vec<Committed>,
}

struct DurableState {
    template: ClaimedMessage,
    now: Duration,
    snapshot: Snapshot,
}

impl DurableState {
    fn require_current_claim(
        &self,
        claim: &ClaimedMessage,
        owner: &str,
    ) -> Result<(), OutboxStoreError> {
        if claim.message_source() == self.template.message_source()
            && claim.message_id() == self.template.message_id()
            && matches!(
                &self.snapshot.row,
                RowState::Leased { owner: current, generation, expires_at }
                    if current == owner
                        && *generation == claim.lease_generation().get()
                        && self.now < *expires_at
            )
        {
            Ok(())
        } else {
            Err(OutboxStoreError::new(OutboxStoreErrorKind::StateTransition))
        }
    }
}

struct DurableStore {
    state: Arc<Mutex<DurableState>>,
    invocations: Arc<Mutex<Vec<Operation>>>,
    pause: Option<Pause>,
}

#[derive(Clone, Copy)]
enum Completion {
    Published,
    Retry(OutboxRetryDelay, FailureCode),
    Quarantine(FailureCode),
}

impl Completion {
    const fn operation(self) -> Operation {
        match self {
            Self::Published => Operation::Published,
            Self::Retry(..) => Operation::Retry,
            Self::Quarantine(_) => Operation::Quarantine,
        }
    }
}

impl DurableStore {
    fn new() -> Result<Self, Box<dyn Error>> {
        let envelope = COMMAND.build(
            MessageMetadata {
                id: "cancellation-message-01".to_owned(),
                source: Component::Gateway.source_uri().to_owned(),
                message_type: COMMAND.message_type().to_owned(),
                subject: "order/cancellation-order-01".to_owned(),
                time: "2026-10-10T00:00:00Z".to_owned(),
                data_schema: COMMAND.data_schema().to_owned(),
                correlation_id: "cancellation-correlation-01".to_owned(),
                causation_id: "cancellation-request-01".to_owned(),
                idempotency_key: "cancellation-order-01".to_owned(),
                partition_key: "order/cancellation-order-01".to_owned(),
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
                trace_state: None,
            },
            &json!({"mode": "dry_run", "quantity": 1}),
        )?;
        let template = ClaimedMessage::new(
            envelope.source().to_owned(),
            envelope.id().to_owned(),
            envelope.message_type().to_owned(),
            COMMAND.subject()?,
            envelope.to_json()?,
            1,
            LeaseGeneration::new(1)?,
        )?;
        Ok(Self {
            state: Arc::new(Mutex::new(DurableState {
                template,
                now: Duration::ZERO,
                snapshot: Snapshot {
                    row: RowState::Available { at: Duration::ZERO },
                    generation: 0,
                    attempt: 0,
                    journal: Vec::new(),
                },
            })),
            invocations: Arc::new(Mutex::new(Vec::new())),
            pause: None,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, DurableState>, OutboxStoreError> {
        self.state
            .lock()
            .map_err(|_| OutboxStoreError::new(OutboxStoreErrorKind::Invariant))
    }

    fn snapshot(&self) -> Result<Snapshot, OutboxStoreError> {
        Ok(self.lock()?.snapshot.clone())
    }

    fn record_invocation(&self, operation: Operation) {
        self.invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(operation);
    }

    fn invocations(&self) -> Vec<Operation> {
        self.invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn restarted_handle(&self) -> Self {
        // Replacing a worker handle must not reset durable state or its clock.
        Self {
            state: Arc::clone(&self.state),
            invocations: Arc::clone(&self.invocations),
            pause: None,
        }
    }

    fn advance(&self, elapsed: Duration) -> Result<(), OutboxStoreError> {
        self.lock()?.now += elapsed;
        Ok(())
    }

    fn complete<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        owner: &'operation str,
        completion: Completion,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let operation = completion.operation();
            pause_at(self.pause, operation, Boundary::BeforeCommit).await;
            {
                let mut durable = self.lock()?;
                durable.require_current_claim(claim, owner)?;
                let generation = claim.lease_generation().get();
                let (row, committed) = match completion {
                    Completion::Published => {
                        (RowState::Published, Committed::Published(generation))
                    }
                    Completion::Retry(delay, code) => {
                        let at = durable.now + delay.get();
                        (
                            RowState::Available { at },
                            Committed::Retry(generation, at, code),
                        )
                    }
                    Completion::Quarantine(code) => (
                        RowState::Quarantined,
                        Committed::Quarantine(generation, code),
                    ),
                };
                durable.snapshot.row = row;
                durable.snapshot.journal.push(committed);
            }
            pause_at(self.pause, operation, Boundary::AfterCommit).await;
            Ok(())
        })
    }
}

impl OutboxRelayStore for DurableStore {
    fn claim_one<'operation>(
        &'operation mut self,
        lease_owner: &'operation str,
        lease_duration: LeaseDuration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
        // Count the method call even if its returned future is never polled.
        self.record_invocation(Operation::Claim);
        Box::pin(async move {
            pause_at(self.pause, Operation::Claim, Boundary::BeforeCommit).await;
            let claim = {
                let mut durable = self.lock()?;
                let eligible = match &durable.snapshot.row {
                    RowState::Available { at } => durable.now >= *at,
                    RowState::Leased { expires_at, .. } => durable.now >= *expires_at,
                    RowState::Published | RowState::Quarantined => false,
                };
                if !eligible {
                    return Ok(None);
                }
                durable.snapshot.generation += 1;
                durable.snapshot.attempt += 1;
                let generation = durable.snapshot.generation;
                durable.snapshot.row = RowState::Leased {
                    owner: lease_owner.to_owned(),
                    generation,
                    expires_at: durable.now + lease_duration.get(),
                };
                durable.snapshot.journal.push(Committed::Claim(generation));
                let template = &durable.template;
                ClaimedMessage::new(
                    template.message_source().to_owned(),
                    template.message_id().to_owned(),
                    template.message_type().to_owned(),
                    template.transport_subject().to_owned(),
                    template.envelope_bytes().to_vec(),
                    durable.snapshot.attempt,
                    LeaseGeneration::new(generation)?,
                )?
            };
            pause_at(self.pause, Operation::Claim, Boundary::AfterCommit).await;
            Ok(Some(claim))
        })
    }

    fn mark_published<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        self.record_invocation(Operation::Published);
        self.complete(claim, lease_owner, Completion::Published)
    }

    fn release_for_retry<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        self.record_invocation(Operation::Retry);
        self.complete(
            claim,
            lease_owner,
            Completion::Retry(retry_after, failure_code),
        )
    }

    fn quarantine<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        reason: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        self.record_invocation(Operation::Quarantine);
        self.complete(claim, lease_owner, Completion::Quarantine(reason))
    }
}

#[derive(Clone, Eq, PartialEq)]
struct Publication {
    source: String,
    id: String,
    bytes: Vec<u8>,
}

impl Debug for Publication {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Publication")
            .field("envelope_bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum Response {
    Confirm,
    PersistThenPending,
    Fail(PublishErrorKind),
}

#[derive(Default)]
struct PublisherState {
    calls: Vec<Publication>,
    retained: Option<Publication>,
}

struct DurablePublisher {
    state: Mutex<PublisherState>,
    invocations: AtomicUsize,
    response: Response,
}

impl DurablePublisher {
    fn new(response: Response) -> Self {
        Self {
            state: Mutex::new(PublisherState::default()),
            invocations: AtomicUsize::new(0),
            response,
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, PublisherState>, PublishError> {
        self.state
            .lock()
            .map_err(|_| PublishError::new(PublishErrorKind::Unavailable))
    }
}

impl MessagePublisher for DurablePublisher {
    fn publish<'publisher>(
        &'publisher self,
        definition: MessageDefinition,
        envelope: &'publisher MessageEnvelope,
    ) -> PublishFuture<'publisher> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            definition.validate_envelope(envelope)?;
            let publication = Publication {
                source: envelope.source().to_owned(),
                id: envelope.id().to_owned(),
                bytes: envelope.to_json().map_err(|error| {
                    PublishError::with_source(PublishErrorKind::Contract, error)
                })?,
            };
            let disposition = {
                let mut broker = self.lock()?;
                broker.calls.push(publication.clone());
                if let Response::Fail(kind) = self.response {
                    return Err(PublishError::new(kind));
                }
                if broker.retained.as_ref() == Some(&publication) {
                    PublishDisposition::Duplicate
                } else {
                    broker.retained = Some(publication);
                    PublishDisposition::Persisted
                }
            };
            if matches!(self.response, Response::PersistThenPending) {
                pending::<()>().await;
            }
            Ok(PublishReceipt::new(disposition))
        })
    }
}

fn policy() -> Result<RelayPolicy, Box<dyn Error>> {
    Ok(RelayPolicy::new(
        WORKER,
        LEASE,
        5,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )?)
}

fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

fn cancel_pending<F: Future>(mut future: Pin<Box<F>>) {
    assert!(poll_once(future.as_mut()).is_pending());
    drop(future);
}

fn assert_leased(snapshot: &Snapshot, generation: u64) {
    assert!(matches!(
        &snapshot.row,
        RowState::Leased { owner, generation: current, expires_at }
            if owner == WORKER && *current == generation && *expires_at == LEASE
    ));
    assert_eq!(snapshot.journal, [Committed::Claim(generation)]);
}

#[test]
fn cancelled_claim_before_commit_leaves_message_available_and_does_not_publish()
-> Result<(), Box<dyn Error>> {
    let mut store = DurableStore::new()?;
    let before = store.snapshot()?;
    store.pause = Some(Pause(Operation::Claim, Boundary::BeforeCommit));
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let publisher = DurablePublisher::new(Response::Confirm);
    let policy = policy()?;

    cancel_pending(Box::pin(relay_once(
        &mut store, &registry, &publisher, &policy,
    )));

    assert_eq!(store.snapshot()?, before);
    assert_eq!(store.invocations(), [Operation::Claim]);
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 0);
    assert!(publisher.lock()?.calls.is_empty());
    store = store.restarted_handle();
    {
        let mut claim = store.claim_one(WORKER, LeaseDuration::new(LEASE)?);
        assert!(
            matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(Some(message)))
            if message.lease_generation().get() == 1)
        );
    }
    assert_eq!(store.invocations(), [Operation::Claim, Operation::Claim]);
    Ok(())
}

#[test]
fn cancelled_committed_claim_waits_for_expiry_and_reuses_owner_with_new_generation()
-> Result<(), Box<dyn Error>> {
    let mut store = DurableStore::new()?;
    store.pause = Some(Pause(Operation::Claim, Boundary::AfterCommit));
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let publisher = DurablePublisher::new(Response::Confirm);
    let policy = policy()?;

    cancel_pending(Box::pin(relay_once(
        &mut store, &registry, &publisher, &policy,
    )));

    assert_leased(&store.snapshot()?, 1);
    assert_eq!(store.invocations(), [Operation::Claim]);
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 0);
    assert!(publisher.lock()?.calls.is_empty());
    store = store.restarted_handle();
    {
        let mut claim = store.claim_one(WORKER, LeaseDuration::new(LEASE)?);
        assert!(matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(None))));
    }
    store.advance(LEASE)?;
    {
        let mut claim = store.claim_one(WORKER, LeaseDuration::new(LEASE)?);
        assert!(
            matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(Some(message)))
            if message.lease_generation().get() == 2 && message.attempt() == 2)
        );
    }
    assert_eq!(
        store.invocations(),
        [Operation::Claim, Operation::Claim, Operation::Claim]
    );
    Ok(())
}

#[test]
fn cancelled_persisted_publish_retries_identical_message_then_records_duplicate_receipt()
-> Result<(), Box<dyn Error>> {
    let mut store = DurableStore::new()?;
    let stored_bytes = store.lock()?.template.envelope_bytes().to_vec();
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let mut publisher = DurablePublisher::new(Response::PersistThenPending);
    let policy = policy()?;

    cancel_pending(Box::pin(relay_once(
        &mut store, &registry, &publisher, &policy,
    )));

    assert_leased(&store.snapshot()?, 1);
    assert_eq!(store.invocations(), [Operation::Claim]);
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 1);
    assert_eq!(publisher.lock()?.calls.len(), 1);
    assert!(publisher.lock()?.retained.is_some());
    store = store.restarted_handle();
    {
        let mut retry = Box::pin(relay_once(&mut store, &registry, &publisher, &policy));
        assert!(matches!(
            poll_once(retry.as_mut()),
            Poll::Ready(Ok(RelayOutcome::Idle))
        ));
    }
    assert_eq!(store.invocations(), [Operation::Claim, Operation::Claim]);
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 1);
    assert_eq!(publisher.lock()?.calls.len(), 1);
    store.advance(LEASE)?;
    publisher.response = Response::Confirm;
    {
        let mut retry = Box::pin(relay_once(&mut store, &registry, &publisher, &policy));
        assert!(matches!(
            poll_once(retry.as_mut()),
            Poll::Ready(Ok(RelayOutcome::Published {
                disposition: PublishDisposition::Duplicate,
                attempt: 2,
            }))
        ));
    }
    let snapshot = store.snapshot()?;
    assert_eq!(
        store.invocations(),
        [
            Operation::Claim,
            Operation::Claim,
            Operation::Claim,
            Operation::Published
        ]
    );
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.row, RowState::Published);
    assert_eq!(
        snapshot.journal,
        [
            Committed::Claim(1),
            Committed::Claim(2),
            Committed::Published(2)
        ]
    );
    let broker = publisher.lock()?;
    assert_eq!(broker.calls.len(), 2);
    let mut calls = broker.calls.iter();
    let first = calls.next().ok_or("missing first publication")?;
    let retry = calls.next().ok_or("missing retry publication")?;
    assert_eq!(first, retry);
    assert!(first.bytes == stored_bytes);
    assert_eq!(broker.retained.as_ref(), Some(first));
    // This models broker receipt deduplication only; durable consumer inbox
    // deduplication remains necessary and is not simulated by this publisher.
    Ok(())
}

#[test]
fn cancelled_outcome_commit_preserves_durable_state_without_alternative_transition()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = policy()?;
    for (operation, response) in [
        (Operation::Published, Response::Confirm),
        (
            Operation::Retry,
            Response::Fail(PublishErrorKind::Unavailable),
        ),
        (
            Operation::Quarantine,
            Response::Fail(PublishErrorKind::Rejected),
        ),
    ] {
        for boundary in [Boundary::BeforeCommit, Boundary::AfterCommit] {
            let mut store = DurableStore::new()?;
            store.pause = Some(Pause(operation, boundary));
            let publisher = DurablePublisher::new(response);

            cancel_pending(Box::pin(relay_once(
                &mut store, &registry, &publisher, &policy,
            )));

            let snapshot = store.snapshot()?;
            assert_eq!(store.invocations(), [Operation::Claim, operation]);
            assert_eq!(publisher.invocations.load(Ordering::SeqCst), 1);
            assert_eq!(publisher.lock()?.calls.len(), 1);
            if boundary == Boundary::BeforeCommit {
                assert_leased(&snapshot, 1);
            } else {
                assert_eq!(snapshot.journal.len(), 2);
                match operation {
                    Operation::Published => {
                        assert_eq!(snapshot.row, RowState::Published);
                        assert_eq!(snapshot.journal.last(), Some(&Committed::Published(1)));
                    }
                    Operation::Retry => {
                        assert!(matches!(snapshot.row, RowState::Available { at }
                            if at > Duration::ZERO && at <= Duration::from_secs(1)));
                        assert!(
                            matches!(snapshot.journal.last(), Some(Committed::Retry(1, _, code))
                            if code.as_str() == "transport_unavailable")
                        );
                    }
                    Operation::Quarantine => {
                        assert_eq!(snapshot.row, RowState::Quarantined);
                        assert!(
                            matches!(snapshot.journal.last(), Some(Committed::Quarantine(1, code))
                            if code.as_str() == "transport_rejected")
                        );
                    }
                    Operation::Claim => return Err("unexpected completion operation".into()),
                }
            }
            // Inspect persistence before recovery: dropping the relay is not
            // an instruction to retry, quarantine, or mark another outcome.
            store = store.restarted_handle();
            {
                let mut claim = store.claim_one(WORKER, LeaseDuration::new(LEASE)?);
                assert!(matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(None))));
            }
            store.advance(LEASE)?;
            let recoverable = boundary == Boundary::BeforeCommit || operation == Operation::Retry;
            {
                let mut claim = store.claim_one(WORKER, LeaseDuration::new(LEASE)?);
                if recoverable {
                    assert!(
                        matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(Some(message)))
                        if message.lease_generation().get() == 2)
                    );
                } else {
                    assert!(matches!(poll_once(claim.as_mut()), Poll::Ready(Ok(None))));
                }
            }
            assert_eq!(
                store.invocations(),
                [
                    Operation::Claim,
                    operation,
                    Operation::Claim,
                    Operation::Claim
                ]
            );
            assert_eq!(publisher.invocations.load(Ordering::SeqCst), 1);
        }
    }
    Ok(())
}

#[test]
fn invocation_probes_record_unpolled_futures_without_recording_durable_effects()
-> Result<(), Box<dyn Error>> {
    let mut store = DurableStore::new()?;
    let before = store.snapshot()?;
    let claim = store.lock()?.template.clone();
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let envelope = claim.validated_envelope(&registry)?;
    let publisher = DurablePublisher::new(Response::Confirm);

    drop(store.claim_one(WORKER, LeaseDuration::new(LEASE)?));
    drop(store.mark_published(&claim, WORKER));
    drop(store.release_for_retry(
        &claim,
        WORKER,
        OutboxRetryDelay::IMMEDIATE,
        FailureCode::from_static("transport_unavailable"),
    ));
    drop(store.quarantine(
        &claim,
        WORKER,
        FailureCode::from_static("transport_rejected"),
    ));
    drop(publisher.publish(COMMAND, &envelope));

    assert_eq!(
        store.invocations(),
        [
            Operation::Claim,
            Operation::Published,
            Operation::Retry,
            Operation::Quarantine
        ]
    );
    assert_eq!(store.snapshot()?, before);
    assert_eq!(publisher.invocations.load(Ordering::SeqCst), 1);
    assert!(publisher.lock()?.calls.is_empty());
    assert!(publisher.lock()?.retained.is_none());
    Ok(())
}
