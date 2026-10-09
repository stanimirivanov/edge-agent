use crate::retry::{RetryDecision, retry_decision, retry_delay};
use crate::{QuarantineReason, RelayErrorKind, RelayOutcome, RelayPolicy, relay_once};

use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRegistry};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, LeaseGeneration, MessagePublisher,
    OutboxRelayStore, OutboxRetryDelay, OutboxStoreFuture, PublishDisposition, PublishError,
    PublishErrorKind, PublishFuture, PublishReceipt,
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
    Claimed(LeaseDuration),
    Published(u64),
    RetryScheduled(u64, OutboxRetryDelay, &'static str),
    Quarantined(u64, &'static str),
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
        lease_duration: LeaseDuration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
        Box::pin(async move {
            self.transitions.push(Transition::Claimed(lease_duration));
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
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            self.transitions.push(Transition::RetryScheduled(
                claim.lease_generation().get(),
                retry_after,
                failure_code.as_str(),
            ));
            Ok(())
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        _lease_owner: &'operation str,
        reason: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            self.transitions.push(Transition::Quarantined(
                claim.lease_generation().get(),
                reason.as_str(),
            ));
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
        [
            Transition::Claimed(LeaseDuration::new(Duration::from_secs(30))?),
            Transition::Published(CLAIM_GENERATION)
        ]
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
    let expected_delay = retry_delay(
        &policy,
        Component::Gateway.source_uri(),
        "relay-port-message-01",
        1,
    );
    assert!(matches!(
        retry,
        RelayOutcome::RetryScheduled { attempt: 1, delay, .. } if delay == expected_delay
    ));
    assert_eq!(
        retry_store.transitions,
        [
            Transition::Claimed(LeaseDuration::new(Duration::from_secs(30))?),
            Transition::RetryScheduled(
                CLAIM_GENERATION,
                OutboxRetryDelay::new(expected_delay)?,
                "transport_unavailable"
            )
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
            Transition::Claimed(LeaseDuration::new(Duration::from_secs(30))?),
            Transition::Quarantined(CLAIM_GENERATION, "transport_rejected")
        ]
    );
    Ok(())
}

#[tokio::test]
async fn relay_forwards_maximum_lease_and_bounded_retry_to_the_store() -> Result<(), Box<dyn Error>>
{
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "relay_port_test",
        LeaseDuration::MAX,
        100,
        OutboxRetryDelay::MAX,
        OutboxRetryDelay::MAX,
    )?;
    let mut store = InMemoryStore {
        claimed: Some(claimed_message()?),
        ..InMemoryStore::default()
    };
    let expected_delay = retry_delay(
        &policy,
        Component::Gateway.source_uri(),
        "relay-port-message-01",
        1,
    );

    let outcome = relay_once(
        &mut store,
        &registry,
        &FailingPublisher(PublishErrorKind::Unavailable),
        &policy,
    )
    .await?;

    assert!(matches!(
        outcome,
        RelayOutcome::RetryScheduled { delay, .. } if delay == expected_delay
    ));
    assert_eq!(
        store.transitions,
        [
            Transition::Claimed(LeaseDuration::new(LeaseDuration::MAX)?),
            Transition::RetryScheduled(
                CLAIM_GENERATION,
                OutboxRetryDelay::new(expected_delay)?,
                "transport_unavailable"
            )
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
fn policy_uses_port_timing_bounds_and_retains_stricter_backoff_minimum() {
    for lease_duration in [LeaseDuration::MIN, LeaseDuration::MAX] {
        assert!(
            RelayPolicy::new(
                "relay_01",
                lease_duration,
                100,
                Duration::from_millis(1),
                OutboxRetryDelay::MAX,
            )
            .is_ok()
        );
    }

    for (lease_duration, base_retry_delay, max_retry_delay) in [
        (
            LeaseDuration::MIN - Duration::from_nanos(1),
            Duration::from_millis(1),
            OutboxRetryDelay::MAX,
        ),
        (
            LeaseDuration::MAX + Duration::from_nanos(1),
            Duration::from_millis(1),
            OutboxRetryDelay::MAX,
        ),
        (LeaseDuration::MIN, Duration::ZERO, OutboxRetryDelay::MAX),
        (
            LeaseDuration::MIN,
            Duration::from_nanos(999_999),
            OutboxRetryDelay::MAX,
        ),
        (
            LeaseDuration::MIN,
            Duration::from_millis(1),
            OutboxRetryDelay::MAX + Duration::from_nanos(1),
        ),
    ] {
        assert_eq!(
            RelayPolicy::new(
                "relay_01",
                lease_duration,
                3,
                base_retry_delay,
                max_retry_delay,
            )
            .err()
            .map(|error| error.kind()),
            Some(RelayErrorKind::InvalidPolicy)
        );
    }
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
            failure_code,
            ..
        } if failure_code.as_str() == "transport_unavailable"
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
fn exponential_backoff_clamps_before_jitter_and_stays_within_port_bounds()
-> Result<(), Box<dyn Error>> {
    let policy = RelayPolicy::new(
        "relay_01",
        LeaseDuration::MAX,
        100,
        Duration::from_secs(2),
        Duration::from_secs(5),
    )?;
    for (attempt, exponential_millis) in [
        (1, 2_000_u64),
        (2, 4_000),
        (3, 5_000),
        (63, 5_000),
        (64, 5_000),
        (100, 5_000),
        (u32::MAX, 5_000),
    ] {
        let delay = retry_delay(&policy, "urn:edgeagent:gateway", "message-01", attempt);
        assert!(delay >= Duration::from_millis(exponential_millis.div_ceil(2)));
        assert!(delay <= Duration::from_millis(exponential_millis));
        assert_eq!(OutboxRetryDelay::new(delay)?.get(), delay);
    }

    for limit in [Duration::from_millis(1), OutboxRetryDelay::MAX] {
        let policy = RelayPolicy::new("relay_01", LeaseDuration::MIN, 100, limit, limit)?;
        let delay = retry_delay(&policy, "urn:edgeagent:gateway", "message-01", u32::MAX);
        assert!(delay >= limit / 2);
        assert!(delay <= limit);
        assert_eq!(OutboxRetryDelay::new(delay)?.get(), delay);
        if limit == Duration::from_millis(1) {
            assert_eq!(delay, limit);
        }
    }
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
