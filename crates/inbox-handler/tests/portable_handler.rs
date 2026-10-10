//! Credential-free conformance for persistence-neutral inbound coordination.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_handler::{
    HandlerErrorKind, HandlerFailure, HandlerPolicy, HandlingOutcome, QuarantineDisposition,
    handle_once,
};
use edgeagent_messaging::{
    ConsumeError, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata,
    DeliverySettlement, DeliverySubject, FailureCode, InboundMessageStore, InboundProcessingError,
    InboundQuarantine, InboxDisposition, InboxFuture, InboxStoreError, InboxStoreErrorKind,
    MessageDelivery, SettlementFuture,
};
use serde_json::json;
use std::collections::{HashMap, hash_map::Entry};
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type EventTrace = Arc<Mutex<Vec<&'static str>>>;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

#[derive(Clone, Copy)]
enum ProcessBehavior {
    Applied,
    Duplicate,
    TransientHandlerFailure,
    PermanentHandlerFailure,
    Contract,
    Unavailable,
    IdentityConflict,
    Invariant,
}

#[derive(Clone, Copy)]
enum QuarantineBehavior {
    Available,
    Unavailable,
    CommitThenUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedQuarantine {
    delivery_key: String,
    transport_subject: String,
    delivery_attempt: u32,
    payload: Vec<u8>,
    failure_code: String,
}

impl RecordedQuarantine {
    fn same_immutable_evidence(&self, other: &Self) -> bool {
        self.delivery_key == other.delivery_key
            && self.transport_subject == other.transport_subject
            && self.payload == other.payload
            && self.failure_code == other.failure_code
    }
}

struct ScriptedStore {
    process_behavior: ProcessBehavior,
    quarantine_behavior: QuarantineBehavior,
    process_calls: usize,
    quarantines: Vec<RecordedQuarantine>,
    retained_quarantines: HashMap<(String, String), RecordedQuarantine>,
    events: Option<EventTrace>,
}

impl ScriptedStore {
    fn new(process_behavior: ProcessBehavior, quarantine_behavior: QuarantineBehavior) -> Self {
        Self {
            process_behavior,
            quarantine_behavior,
            process_calls: 0,
            quarantines: Vec::new(),
            retained_quarantines: HashMap::new(),
            events: None,
        }
    }

    fn with_events(mut self, events: EventTrace) -> Self {
        self.events = Some(events);
        self
    }

    fn record_event(&self, event: &'static str) -> Result<(), InboxStoreError> {
        if let Some(events) = &self.events {
            events
                .lock()
                .map_err(|_| {
                    InboxStoreError::with_source(
                        InboxStoreErrorKind::Invariant,
                        io::Error::other("event trace lock was poisoned"),
                    )
                })?
                .push(event);
        }
        Ok(())
    }

    fn retain_quarantine(
        &mut self,
        consumer_name: &str,
        recorded: RecordedQuarantine,
    ) -> Result<QuarantineDisposition, InboxStoreError> {
        let key = (consumer_name.to_owned(), recorded.delivery_key.clone());
        match self.retained_quarantines.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(recorded);
                Ok(QuarantineDisposition::Inserted)
            }
            Entry::Occupied(entry) if entry.get().same_immutable_evidence(&recorded) => {
                Ok(QuarantineDisposition::AlreadyPresent)
            }
            Entry::Occupied(_) => Err(InboxStoreError::new(InboxStoreErrorKind::Invariant)),
        }
    }
}

impl InboundMessageStore for ScriptedStore {
    fn process<'operation>(
        &'operation mut self,
        _consumer_name: &'operation str,
        _registry: &'operation MessageRegistry<'_>,
        _envelope: &'operation MessageEnvelope,
    ) -> InboxFuture<'operation, Result<InboxDisposition, InboundProcessingError>> {
        Box::pin(async move {
            self.process_calls += 1;
            match self.process_behavior {
                ProcessBehavior::Applied => Ok(InboxDisposition::Applied),
                ProcessBehavior::Duplicate => Ok(InboxDisposition::Duplicate),
                ProcessBehavior::TransientHandlerFailure => Err(InboundProcessingError::Handler(
                    HandlerFailure::transient(FailureCode::from_static("dependency_unavailable")),
                )),
                ProcessBehavior::PermanentHandlerFailure => Err(InboundProcessingError::Handler(
                    HandlerFailure::permanent(FailureCode::from_static("policy_rejected")),
                )),
                ProcessBehavior::Contract => Err(store_error(InboxStoreErrorKind::Contract)),
                ProcessBehavior::Unavailable => Err(store_error(InboxStoreErrorKind::Unavailable)),
                ProcessBehavior::IdentityConflict => {
                    Err(store_error(InboxStoreErrorKind::MessageIdentityConflict))
                }
                ProcessBehavior::Invariant => Err(store_error(InboxStoreErrorKind::Invariant)),
            }
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        evidence: InboundQuarantine<'operation>,
    ) -> InboxFuture<'operation, Result<QuarantineDisposition, InboxStoreError>> {
        Box::pin(async move {
            let recorded = RecordedQuarantine {
                delivery_key: evidence.delivery_key().to_owned(),
                transport_subject: evidence.transport_subject().to_owned(),
                delivery_attempt: evidence.delivery_attempt(),
                payload: evidence.payload().to_vec(),
                failure_code: evidence.failure_code().to_owned(),
            };
            self.quarantines.push(recorded.clone());
            let result = match self.quarantine_behavior {
                QuarantineBehavior::Available => self.retain_quarantine(consumer_name, recorded),
                QuarantineBehavior::Unavailable => {
                    Err(InboxStoreError::new(InboxStoreErrorKind::Unavailable))
                }
                QuarantineBehavior::CommitThenUnavailable => {
                    match self.retain_quarantine(consumer_name, recorded) {
                        Ok(_) => Err(InboxStoreError::new(InboxStoreErrorKind::Unavailable)),
                        Err(error) => Err(error),
                    }
                }
            };
            self.record_event(if result.is_ok() {
                "quarantine:committed"
            } else {
                "quarantine:failed"
            })?;
            result
        })
    }
}

fn store_error(kind: InboxStoreErrorKind) -> InboundProcessingError {
    InboundProcessingError::Store(InboxStoreError::new(kind))
}

struct SettlementProbe {
    dispositions: Arc<Mutex<Vec<DeliveryDisposition>>>,
    fail_confirmation: bool,
    events: Option<EventTrace>,
}

impl DeliverySettlement for SettlementProbe {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async move {
            if let Some(events) = &self.events {
                let event = match &disposition {
                    DeliveryDisposition::Acknowledge => "settle:acknowledge",
                    DeliveryDisposition::RetryAfter(_) => "settle:retry",
                    DeliveryDisposition::Quarantined => "settle:quarantined",
                };
                events
                    .lock()
                    .map_err(|_| ConsumeError::unavailable("event trace lock was poisoned"))?
                    .push(event);
            }
            self.dispositions
                .lock()
                .map_err(|_| ConsumeError::unavailable("settlement probe lock was poisoned"))?
                .push(disposition);
            if self.fail_confirmation {
                Err(ConsumeError::unavailable(
                    "settlement probe confirmation failed",
                ))
            } else {
                Ok(())
            }
        })
    }
}

fn envelope(message_id: &str) -> Result<MessageEnvelope, Box<dyn Error>> {
    Ok(COMMAND.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: format!("order/{message_id}"),
            time: "2026-09-29T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: format!("correlation-{message_id}"),
            causation_id: format!("request-{message_id}"),
            idempotency_key: message_id.to_owned(),
            partition_key: format!("order/{message_id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run"}),
    )?)
}

fn delivery(
    payload: Vec<u8>,
    attempt: u32,
    dispositions: Arc<Mutex<Vec<DeliveryDisposition>>>,
    fail_confirmation: bool,
) -> Result<MessageDelivery, Box<dyn Error>> {
    delivery_with_events(payload, attempt, dispositions, fail_confirmation, None)
}

fn delivery_with_events(
    payload: Vec<u8>,
    attempt: u32,
    dispositions: Arc<Mutex<Vec<DeliveryDisposition>>>,
    fail_confirmation: bool,
    events: Option<EventTrace>,
) -> Result<MessageDelivery, Box<dyn Error>> {
    Ok(MessageDelivery::new(
        payload,
        DeliveryMetadata::new(
            DeliveryMessageKey::new("18:EDGEAGENT_COMMANDS:41")?,
            DeliverySubject::new(COMMAND.subject()?)?,
            DeliveryAttempt::new(attempt)?,
        ),
        Box::new(SettlementProbe {
            dispositions,
            fail_confirmation,
            events,
        }),
    )?)
}

fn policy() -> Result<HandlerPolicy, Box<dyn Error>> {
    Ok(HandlerPolicy::new(
        "execution_simulator_v1",
        3,
        Duration::from_millis(2),
        Duration::from_millis(20),
    )?)
}

fn observed(
    dispositions: &Arc<Mutex<Vec<DeliveryDisposition>>>,
) -> Result<Vec<DeliveryDisposition>, Box<dyn Error>> {
    Ok(dispositions
        .lock()
        .map_err(|_| io::Error::other("settlement probe lock was poisoned"))?
        .clone())
}

fn observed_events(events: &EventTrace) -> Result<Vec<&'static str>, Box<dyn Error>> {
    Ok(events
        .lock()
        .map_err(|_| io::Error::other("event trace lock was poisoned"))?
        .clone())
}

#[tokio::test]
async fn applied_and_duplicate_results_acknowledge_without_adapter_knowledge()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;

    for (behavior, message_id) in [
        (ProcessBehavior::Applied, "applied"),
        (ProcessBehavior::Duplicate, "duplicate"),
    ] {
        let settlements = Arc::new(Mutex::new(Vec::new()));
        let mut store = ScriptedStore::new(behavior, QuarantineBehavior::Available);
        let outcome = handle_once(
            &mut store,
            &registry,
            &policy()?,
            delivery(
                envelope(message_id)?.to_json()?,
                1,
                Arc::clone(&settlements),
                false,
            )?,
        )
        .await?;

        match behavior {
            ProcessBehavior::Applied => {
                assert!(matches!(outcome, HandlingOutcome::Applied { attempt: 1 }));
            }
            ProcessBehavior::Duplicate => {
                assert!(matches!(outcome, HandlingOutcome::Duplicate { attempt: 1 }));
            }
            _ => unreachable!("test table contains only committed dispositions"),
        }
        assert_eq!(store.process_calls, 1);
        assert!(store.quarantines.is_empty());
        assert_eq!(
            observed(&settlements)?,
            vec![DeliveryDisposition::Acknowledge]
        );
    }
    Ok(())
}

#[tokio::test]
async fn transient_handler_and_store_outages_request_redelivery() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;

    for behavior in [
        ProcessBehavior::TransientHandlerFailure,
        ProcessBehavior::Unavailable,
    ] {
        let settlements = Arc::new(Mutex::new(Vec::new()));
        let mut store = ScriptedStore::new(behavior, QuarantineBehavior::Available);
        let outcome = handle_once(
            &mut store,
            &registry,
            &policy()?,
            delivery(
                envelope("retry-01")?.to_json()?,
                1,
                Arc::clone(&settlements),
                false,
            )?,
        )
        .await?;

        assert!(matches!(
            outcome,
            HandlingOutcome::RetryRequested { attempt: 1, .. }
        ));
        assert!(matches!(
            observed(&settlements)?.as_slice(),
            [DeliveryDisposition::RetryAfter(_)]
        ));
        assert!(store.quarantines.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn exhausted_transient_failure_commits_quarantine_instead_of_retrying()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(
        ProcessBehavior::TransientHandlerFailure,
        QuarantineBehavior::Available,
    );

    let outcome = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(
            envelope("retry-exhausted-01")?.to_json()?,
            3,
            Arc::clone(&settlements),
            false,
        )?,
    )
    .await?;

    assert!(matches!(
        outcome,
        HandlingOutcome::Quarantined {
            attempt: 3,
            disposition: QuarantineDisposition::Inserted,
            failure_code: "dependency_unavailable",
            ..
        }
    ));
    assert_eq!(store.quarantines.len(), 1);
    assert_eq!(store.quarantines[0].delivery_attempt, 3);
    assert_eq!(store.quarantines[0].failure_code, "dependency_unavailable");
    assert_eq!(
        observed(&settlements)?,
        vec![DeliveryDisposition::Quarantined]
    );
    Ok(())
}

#[tokio::test]
async fn terminal_failure_commits_exact_quarantine_evidence_before_settlement()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(
        ProcessBehavior::PermanentHandlerFailure,
        QuarantineBehavior::Available,
    )
    .with_events(Arc::clone(&events));
    let payload = envelope("permanent-01")?.to_json()?;

    let outcome = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery_with_events(
            payload.clone(),
            2,
            Arc::clone(&settlements),
            false,
            Some(Arc::clone(&events)),
        )?,
    )
    .await?;

    assert!(matches!(
        outcome,
        HandlingOutcome::Quarantined {
            attempt: 2,
            disposition: QuarantineDisposition::Inserted,
            failure_code: "policy_rejected",
            ..
        }
    ));
    assert_eq!(
        store.quarantines,
        vec![RecordedQuarantine {
            delivery_key: "18:EDGEAGENT_COMMANDS:41".to_owned(),
            transport_subject: COMMAND.subject()?.to_owned(),
            delivery_attempt: 2,
            payload,
            failure_code: "policy_rejected".to_owned(),
        }]
    );
    assert_eq!(
        observed(&settlements)?,
        vec![DeliveryDisposition::Quarantined]
    );
    assert_eq!(
        observed_events(&events)?,
        vec!["quarantine:committed", "settle:quarantined"]
    );
    Ok(())
}

#[tokio::test]
async fn store_contract_rejection_commits_routing_evidence_before_terminal_settlement()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(ProcessBehavior::Contract, QuarantineBehavior::Available)
        .with_events(Arc::clone(&events));

    let outcome = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery_with_events(
            envelope("contract-01")?.to_json()?,
            1,
            Arc::clone(&settlements),
            false,
            Some(Arc::clone(&events)),
        )?,
    )
    .await?;

    assert!(matches!(
        outcome,
        HandlingOutcome::Quarantined {
            failure_code: "routing_invalid",
            ..
        }
    ));
    assert_eq!(store.process_calls, 1);
    assert_eq!(store.quarantines.len(), 1);
    assert_eq!(store.quarantines[0].failure_code, "routing_invalid");
    assert!(matches!(
        observed(&settlements)?.as_slice(),
        [DeliveryDisposition::Quarantined]
    ));
    assert_eq!(
        observed_events(&events)?,
        vec!["quarantine:committed", "settle:quarantined"]
    );
    Ok(())
}

#[tokio::test]
async fn identity_conflicts_are_quarantined_with_a_stable_reason() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(
        ProcessBehavior::IdentityConflict,
        QuarantineBehavior::Available,
    );

    let outcome = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(
            envelope("conflict-01")?.to_json()?,
            1,
            Arc::clone(&settlements),
            false,
        )?,
    )
    .await?;

    assert!(matches!(
        outcome,
        HandlingOutcome::Quarantined {
            failure_code: "message_identity_conflict",
            ..
        }
    ));
    assert_eq!(
        store.quarantines[0].failure_code,
        "message_identity_conflict"
    );
    Ok(())
}

#[tokio::test]
async fn failed_quarantine_persistence_retries_instead_of_terminally_settling()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(ProcessBehavior::Applied, QuarantineBehavior::Unavailable)
        .with_events(Arc::clone(&events));

    let outcome = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery_with_events(
            b"{not-json".to_vec(),
            1,
            Arc::clone(&settlements),
            false,
            Some(Arc::clone(&events)),
        )?,
    )
    .await?;

    assert!(matches!(outcome, HandlingOutcome::RetryRequested { .. }));
    assert_eq!(store.process_calls, 0);
    assert_eq!(store.quarantines.len(), 1);
    assert!(matches!(
        observed(&settlements)?.as_slice(),
        [DeliveryDisposition::RetryAfter(_)]
    ));
    assert_eq!(
        observed_events(&events)?,
        vec!["quarantine:failed", "settle:retry"]
    );
    Ok(())
}

#[tokio::test]
async fn invariant_and_settlement_failures_remain_hard_errors() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;

    let invariant_settlements = Arc::new(Mutex::new(Vec::new()));
    let mut invariant_store =
        ScriptedStore::new(ProcessBehavior::Invariant, QuarantineBehavior::Available);
    let invariant = handle_once(
        &mut invariant_store,
        &registry,
        &policy()?,
        delivery(
            envelope("invariant-01")?.to_json()?,
            1,
            Arc::clone(&invariant_settlements),
            false,
        )?,
    )
    .await;
    assert_eq!(
        invariant.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Inbox)
    );
    assert!(observed(&invariant_settlements)?.is_empty());

    for (behavior, message_id) in [
        (ProcessBehavior::Applied, "applied-ack-loss"),
        (ProcessBehavior::Duplicate, "duplicate-ack-loss"),
    ] {
        let failed_ack_settlements = Arc::new(Mutex::new(Vec::new()));
        let mut store = ScriptedStore::new(behavior, QuarantineBehavior::Available);
        let failed_ack = handle_once(
            &mut store,
            &registry,
            &policy()?,
            delivery(
                envelope(message_id)?.to_json()?,
                1,
                Arc::clone(&failed_ack_settlements),
                true,
            )?,
        )
        .await;
        assert_eq!(
            failed_ack.err().map(|error| error.kind()),
            Some(HandlerErrorKind::Settlement)
        );
        assert_eq!(store.process_calls, 1);
        assert_eq!(
            observed(&failed_ack_settlements)?,
            vec![DeliveryDisposition::Acknowledge]
        );
    }
    Ok(())
}

#[tokio::test]
async fn retry_and_quarantine_confirmation_loss_never_report_a_confirmed_outcome()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;

    for (behavior, message_id) in [
        (ProcessBehavior::TransientHandlerFailure, "retry-loss-01"),
        (ProcessBehavior::PermanentHandlerFailure, "terminal-loss-01"),
    ] {
        let settlements = Arc::new(Mutex::new(Vec::new()));
        let mut store = ScriptedStore::new(behavior, QuarantineBehavior::Available);
        let result = handle_once(
            &mut store,
            &registry,
            &policy()?,
            delivery(
                envelope(message_id)?.to_json()?,
                1,
                Arc::clone(&settlements),
                true,
            )?,
        )
        .await;

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(HandlerErrorKind::Settlement)
        );
        match behavior {
            ProcessBehavior::TransientHandlerFailure => {
                assert!(matches!(
                    observed(&settlements)?.as_slice(),
                    [DeliveryDisposition::RetryAfter(_)]
                ));
                assert!(store.quarantines.is_empty());
            }
            ProcessBehavior::PermanentHandlerFailure => {
                assert_eq!(
                    observed(&settlements)?,
                    vec![DeliveryDisposition::Quarantined]
                );
                assert_eq!(store.quarantines.len(), 1);
            }
            _ => unreachable!("test table contains only retry and terminal failures"),
        }
    }
    Ok(())
}

#[tokio::test]
async fn lost_terminal_confirmation_reuses_exact_quarantine_evidence_on_redelivery()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(ProcessBehavior::Applied, QuarantineBehavior::Available);
    let payload = b"{invalid-json".to_vec();

    let first = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(payload.clone(), 1, Arc::clone(&settlements), true)?,
    )
    .await;
    assert_eq!(
        first.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Settlement)
    );
    assert_eq!(store.retained_quarantines.len(), 1);

    let second = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(payload.clone(), 2, Arc::clone(&settlements), false)?,
    )
    .await?;
    assert!(matches!(
        second,
        HandlingOutcome::Quarantined {
            attempt: 2,
            disposition: QuarantineDisposition::AlreadyPresent,
            failure_code: "envelope_invalid",
            ..
        }
    ));
    assert_eq!(store.process_calls, 0);
    assert_eq!(store.retained_quarantines.len(), 1);
    assert_eq!(store.quarantines.len(), 2);
    assert_eq!(store.quarantines[0].payload, payload);
    assert_eq!(store.quarantines[1].payload, payload);
    assert_eq!(store.quarantines[0].delivery_attempt, 1);
    assert_eq!(store.quarantines[1].delivery_attempt, 2);
    assert_eq!(
        observed(&settlements)?,
        vec![
            DeliveryDisposition::Quarantined,
            DeliveryDisposition::Quarantined,
        ]
    );
    Ok(())
}

#[tokio::test]
async fn ambiguous_quarantine_commit_retries_then_discovers_retained_evidence()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(
        ProcessBehavior::Applied,
        QuarantineBehavior::CommitThenUnavailable,
    );
    let payload = b"{invalid-json".to_vec();

    let first = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(payload.clone(), 1, Arc::clone(&settlements), false)?,
    )
    .await?;
    assert!(matches!(
        first,
        HandlingOutcome::RetryRequested { attempt: 1, .. }
    ));
    assert_eq!(store.retained_quarantines.len(), 1);
    assert!(matches!(
        observed(&settlements)?.as_slice(),
        [DeliveryDisposition::RetryAfter(_)]
    ));

    store.quarantine_behavior = QuarantineBehavior::Available;
    let second = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(payload, 2, Arc::clone(&settlements), false)?,
    )
    .await?;
    assert!(matches!(
        second,
        HandlingOutcome::Quarantined {
            attempt: 2,
            disposition: QuarantineDisposition::AlreadyPresent,
            ..
        }
    ));
    assert_eq!(store.retained_quarantines.len(), 1);
    assert!(matches!(
        observed(&settlements)?.as_slice(),
        [
            DeliveryDisposition::RetryAfter(_),
            DeliveryDisposition::Quarantined
        ]
    ));
    Ok(())
}

#[tokio::test]
async fn conflicting_quarantine_redelivery_fails_closed_without_settlement()
-> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let settlements = Arc::new(Mutex::new(Vec::new()));
    let mut store = ScriptedStore::new(ProcessBehavior::Applied, QuarantineBehavior::Available);
    let first_payload = b"{first-invalid".to_vec();

    handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(first_payload.clone(), 1, Arc::clone(&settlements), false)?,
    )
    .await?;
    let conflict = handle_once(
        &mut store,
        &registry,
        &policy()?,
        delivery(
            b"{different-invalid".to_vec(),
            2,
            Arc::clone(&settlements),
            false,
        )?,
    )
    .await;

    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Inbox)
    );
    assert_eq!(store.retained_quarantines.len(), 1);
    assert!(
        store
            .retained_quarantines
            .values()
            .all(|e| e.payload.as_slice() == first_payload.as_slice())
    );
    assert_eq!(
        observed(&settlements)?,
        vec![DeliveryDisposition::Quarantined]
    );
    Ok(())
}
