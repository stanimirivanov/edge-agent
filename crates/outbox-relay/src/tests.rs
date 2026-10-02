use crate::retry::{RetryDecision, retry_decision, retry_delay};
use crate::{QuarantineReason, RelayErrorKind, RelayOutcome, RelayPolicy, relay_once};

use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRegistry};
use edgeagent_messaging::{
    ClaimedMessage, LeaseGeneration, MessagePublisher, OutboxRelayStore, OutboxStoreFuture,
    PublishDisposition, PublishError, PublishErrorKind, PublishFuture, PublishReceipt,
};
use serde_json::json;
use std::error::Error;
use std::time::Duration;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const CLAIM_GENERATION: u64 = 7;

#[derive(Debug, Eq, PartialEq)]
enum Transition {
    Claimed,
    Published(u64),
    RetryScheduled(u64),
    Quarantined(u64),
}

#[derive(Default)]
struct InMemoryStore {
    claimed: Option<ClaimedMessage>,
    transitions: Vec<Transition>,
}

impl OutboxRelayStore for InMemoryStore {
    fn claim_one<'operation>(
        &'operation mut self,
        _lease_owner: &'operation str,
        _lease_duration: Duration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
        Box::pin(async move {
            self.transitions.push(Transition::Claimed);
            Ok(self.claimed.take())
        })
    }

    fn mark_published<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        _lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            self.transitions
                .push(Transition::Published(claim.lease_generation().get()));
            Ok(())
        })
    }

    fn release_for_retry<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        _lease_owner: &'operation str,
        _retry_after: Duration,
        _failure_code: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            self.transitions
                .push(Transition::RetryScheduled(claim.lease_generation().get()));
            Ok(())
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        _lease_owner: &'operation str,
        _reason: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            self.transitions
                .push(Transition::Quarantined(claim.lease_generation().get()));
            Ok(())
        })
    }
}

struct PersistingPublisher;

impl MessagePublisher for PersistingPublisher {
    fn publish<'publisher>(
        &'publisher self,
        _definition: MessageDefinition,
        _envelope: &'publisher edgeagent_contracts::MessageEnvelope,
    ) -> PublishFuture<'publisher> {
        Box::pin(async { Ok(PublishReceipt::new(PublishDisposition::Persisted)) })
    }
}

struct FailingPublisher(PublishErrorKind);

impl MessagePublisher for FailingPublisher {
    fn publish<'publisher>(
        &'publisher self,
        _definition: MessageDefinition,
        _envelope: &'publisher edgeagent_contracts::MessageEnvelope,
    ) -> PublishFuture<'publisher> {
        Box::pin(async move {
            Err(PublishError::with_source(
                self.0,
                std::io::Error::other("synthetic publication failure"),
            ))
        })
    }
}

fn claimed_message() -> Result<ClaimedMessage, Box<dyn Error>> {
    let envelope = COMMAND.build(
        MessageMetadata {
            id: "relay-port-message-01".to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: "order/relay-port-order-01".to_owned(),
            time: "2026-09-28T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: "relay-port-correlation-01".to_owned(),
            causation_id: "relay-port-request-01".to_owned(),
            idempotency_key: "relay-port-order-01".to_owned(),
            partition_key: "order/relay-port-order-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run", "quantity": 1}),
    )?;
    Ok(ClaimedMessage::new(
        envelope.source().to_owned(),
        envelope.id().to_owned(),
        envelope.message_type().to_owned(),
        COMMAND.subject()?,
        envelope.to_json()?,
        1,
        LeaseGeneration::new(CLAIM_GENERATION)?,
    )?)
}

#[tokio::test]
async fn relay_orchestration_depends_only_on_application_ports() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "relay_port_test",
        Duration::from_secs(30),
        3,
        Duration::from_secs(1),
        Duration::from_secs(60),
    )?;
    let mut store = InMemoryStore {
        claimed: Some(claimed_message()?),
        ..InMemoryStore::default()
    };

    let outcome = relay_once(&mut store, &registry, &PersistingPublisher, &policy).await?;

    assert!(matches!(
        outcome,
        RelayOutcome::Published {
            disposition: PublishDisposition::Persisted,
            attempt: 1,
        }
    ));
    assert_eq!(
        store.transitions,
        [Transition::Claimed, Transition::Published(CLAIM_GENERATION)]
    );
    Ok(())
}

#[tokio::test]
async fn relay_forwards_claim_generation_for_retry_and_quarantine() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "relay_port_test",
        Duration::from_secs(30),
        3,
        Duration::from_secs(1),
        Duration::from_secs(60),
    )?;

    let mut retry_store = InMemoryStore {
        claimed: Some(claimed_message()?),
        ..InMemoryStore::default()
    };
    let retry = relay_once(
        &mut retry_store,
        &registry,
        &FailingPublisher(PublishErrorKind::Unavailable),
        &policy,
    )
    .await?;
    assert!(matches!(
        retry,
        RelayOutcome::RetryScheduled { attempt: 1, .. }
    ));
    assert_eq!(
        retry_store.transitions,
        [
            Transition::Claimed,
            Transition::RetryScheduled(CLAIM_GENERATION)
        ]
    );

    let mut quarantine_store = InMemoryStore {
        claimed: Some(claimed_message()?),
        ..InMemoryStore::default()
    };
    let quarantined = relay_once(
        &mut quarantine_store,
        &registry,
        &FailingPublisher(PublishErrorKind::Rejected),
        &policy,
    )
    .await?;
    assert!(matches!(
        quarantined,
        RelayOutcome::Quarantined {
            reason: QuarantineReason::TransportRejected,
            attempt: 1,
            ..
        }
    ));
    assert_eq!(
        quarantine_store.transitions,
        [
            Transition::Claimed,
            Transition::Quarantined(CLAIM_GENERATION)
        ]
    );
    Ok(())
}

#[test]
fn policy_bounds_attempts_and_delays() {
    assert_eq!(
        RelayPolicy::new(
            "Relay 01",
            Duration::from_secs(30),
            3,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .err()
        .map(|error| error.kind()),
        Some(RelayErrorKind::InvalidPolicy)
    );
    assert_eq!(
        RelayPolicy::new(
            "relay_01",
            Duration::ZERO,
            3,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .err()
        .map(|error| error.kind()),
        Some(RelayErrorKind::InvalidPolicy)
    );
    assert_eq!(
        RelayPolicy::new(
            "relay_01",
            Duration::from_secs(30),
            0,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .err()
        .map(|error| error.kind()),
        Some(RelayErrorKind::InvalidPolicy)
    );
    assert_eq!(
        RelayPolicy::new(
            "relay_01",
            Duration::from_secs(30),
            3,
            Duration::from_secs(60),
            Duration::from_secs(1),
        )
        .err()
        .map(|error| error.kind()),
        Some(RelayErrorKind::InvalidPolicy)
    );
    assert!(
        RelayPolicy::new(
            "relay_01",
            Duration::from_secs(30),
            3,
            Duration::from_secs(1),
            Duration::from_secs(60),
        )
        .is_ok()
    );
}

#[test]
fn transient_failures_back_off_deterministically_then_stop() -> Result<(), RelayErrorKind> {
    let policy = RelayPolicy::new(
        "relay_01",
        Duration::from_secs(30),
        3,
        Duration::from_secs(2),
        Duration::from_secs(5),
    )
    .map_err(|error| error.kind())?;
    let first = retry_delay(&policy, "urn:edgeagent:gateway", "message-01", 1);
    assert_eq!(
        first,
        retry_delay(&policy, "urn:edgeagent:gateway", "message-01", 1)
    );
    assert!(first >= Duration::from_secs(1));
    assert!(first <= Duration::from_secs(2));
    assert!(matches!(
        retry_decision(
            &policy,
            "urn:edgeagent:gateway",
            "message-01",
            1,
            PublishErrorKind::Unavailable,
        ),
        RetryDecision::Retry {
            failure_code: "transport_unavailable",
            ..
        }
    ));
    assert!(matches!(
        retry_decision(
            &policy,
            "urn:edgeagent:gateway",
            "message-01",
            3,
            PublishErrorKind::ConfirmationUnknown,
        ),
        RetryDecision::Quarantine(QuarantineReason::AttemptsExhaustedConfirmationUnknown)
    ));
    Ok(())
}

#[test]
fn permanent_failures_enter_quarantine_without_retry() -> Result<(), RelayErrorKind> {
    let policy = RelayPolicy::new(
        "relay_01",
        Duration::from_secs(30),
        3,
        Duration::from_secs(1),
        Duration::from_secs(60),
    )
    .map_err(|error| error.kind())?;
    assert!(matches!(
        retry_decision(
            &policy,
            "urn:edgeagent:gateway",
            "message-01",
            1,
            PublishErrorKind::Contract,
        ),
        RetryDecision::Quarantine(QuarantineReason::PublishContract)
    ));
    assert!(matches!(
        retry_decision(
            &policy,
            "urn:edgeagent:gateway",
            "message-01",
            1,
            PublishErrorKind::Rejected,
        ),
        RetryDecision::Quarantine(QuarantineReason::TransportRejected)
    ));
    Ok(())
}
