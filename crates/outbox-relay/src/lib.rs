//! Bounded, persistence-neutral outbox relay orchestration.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageRegistry, MessageRoutingError};
use edgeagent_messaging::{
    ClaimedMessage, MessagePublisher, OutboxRelayStore, OutboxStoreError, OutboxStoreErrorKind,
    PublishDisposition, PublishError, PublishErrorKind,
};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{Duration, Instant};

const MAX_ATTEMPTS: u32 = 100;
const MAX_LEASE_DURATION: Duration = Duration::from_secs(15 * 60);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Bounded relay configuration shared by all iterations of one worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPolicy {
    lease_owner: String,
    lease_duration: Duration,
    max_attempts: u32,
    base_retry_delay: Duration,
    max_retry_delay: Duration,
}

impl RelayPolicy {
    /// Construct a validated policy.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPolicy` when the attempt budget or delay bounds cannot
    /// produce bounded exponential backoff or a safe lease.
    pub fn new(
        lease_owner: impl Into<String>,
        lease_duration: Duration,
        max_attempts: u32,
        base_retry_delay: Duration,
        max_retry_delay: Duration,
    ) -> Result<Self, RelayError> {
        let lease_owner = lease_owner.into();
        if lease_owner.is_empty()
            || lease_owner.len() > 128
            || !lease_owner.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(RelayError::invalid_policy(
                "lease_owner must be a lowercase ASCII token of 1 to 128 bytes",
            ));
        }
        if lease_duration < Duration::from_millis(1) || lease_duration > MAX_LEASE_DURATION {
            return Err(RelayError::invalid_policy(
                "lease_duration must be between 1 millisecond and 15 minutes",
            ));
        }
        if max_attempts == 0 || max_attempts > MAX_ATTEMPTS {
            return Err(RelayError::invalid_policy(
                "max_attempts must be between 1 and 100",
            ));
        }
        if base_retry_delay < Duration::from_millis(1) {
            return Err(RelayError::invalid_policy(
                "base_retry_delay must be at least 1 millisecond",
            ));
        }
        if max_retry_delay < base_retry_delay || max_retry_delay > MAX_RETRY_DELAY {
            return Err(RelayError::invalid_policy(
                "max_retry_delay must be at least the base delay and at most 24 hours",
            ));
        }
        Ok(Self {
            lease_owner,
            lease_duration,
            max_attempts,
            base_retry_delay,
            max_retry_delay,
        })
    }
}

/// Observable durable result of one bounded relay iteration.
#[derive(Debug)]
pub enum RelayOutcome {
    /// No eligible outbox record was available.
    Idle,
    /// Broker persistence was confirmed and the outbox record was marked published.
    Published {
        /// Broker acknowledgement disposition.
        disposition: PublishDisposition,
        /// One-based lease/publication attempt.
        attempt: u32,
    },
    /// A transient or ambiguous failure was durably scheduled for retry.
    RetryScheduled {
        /// Publication failure retained for safe diagnostics and cause inspection.
        failure: PublishError,
        /// One-based lease/publication attempt.
        attempt: u32,
        /// Deterministic jittered delay before the next claim.
        delay: Duration,
    },
    /// The retained record entered terminal quarantine and is no longer claimable.
    Quarantined {
        /// Stable, bounded operator reason.
        reason: QuarantineReason,
        /// One-based lease/publication attempt.
        attempt: u32,
        /// Contract or publication failure retained for diagnostics.
        failure: RelayMessageFailure,
    },
}

/// Handled message failure retained in a durable relay outcome for diagnostics.
#[derive(Debug)]
pub enum RelayMessageFailure {
    /// Stored envelope or denormalized routing metadata failed validation.
    StoredContract(MessageRoutingError),
    /// The portable publisher returned a classified failure.
    Publication(PublishError),
}

impl Display for RelayMessageFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoredContract(_) => {
                formatter.write_str("stored outbox message contract is invalid")
            }
            Self::Publication(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for RelayMessageFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StoredContract(error) => Some(error),
            Self::Publication(error) => Some(error),
        }
    }
}

/// Stable quarantine reasons persisted without broker error text or payload data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineReason {
    /// Stored bytes no longer satisfy the relay's registered contract.
    StoredContractInvalid,
    /// The publisher rejected a definition or envelope contract.
    PublishContract,
    /// The transport explicitly rejected the publication.
    TransportRejected,
    /// Repeated transport unavailability exhausted the attempt budget.
    AttemptsExhaustedUnavailable,
    /// Repeated ambiguous confirmations exhausted the attempt budget.
    AttemptsExhaustedConfirmationUnknown,
}

impl QuarantineReason {
    /// Return the bounded code persisted for operator inspection.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::StoredContractInvalid => "stored_contract_invalid",
            Self::PublishContract => "publish_contract",
            Self::TransportRejected => "transport_rejected",
            Self::AttemptsExhaustedUnavailable => "attempts_exhausted_unavailable",
            Self::AttemptsExhaustedConfirmationUnknown => "attempts_exhausted_confirmation_unknown",
        }
    }
}

/// Claim and resolve at most one outbox record.
///
/// Publication occurs outside persistence transactions. Each worker has at most
/// one in-flight publication, providing an explicit backpressure boundary.
/// Broker confirmation, retry scheduling, or quarantine is then committed in a
/// short transaction guarded by the original lease.
///
/// # Errors
///
/// Returns a storage or outbox error when claiming or durably recording the
/// outcome fails. A publish failure is a normal classified `RelayOutcome`.
pub async fn relay_once(
    store: &mut dyn OutboxRelayStore,
    registry: &MessageRegistry<'_>,
    publisher: &dyn MessagePublisher,
    policy: &RelayPolicy,
) -> Result<RelayOutcome, RelayError> {
    let Some(message) = store
        .claim_one(&policy.lease_owner, policy.lease_duration)
        .await?
    else {
        return Ok(RelayOutcome::Idle);
    };

    let mut telemetry_context = EventSpineContext::from_stored_identity(
        message.message_source(),
        message.message_id(),
        message.message_type(),
    );
    let envelope = match message.validated_envelope(registry) {
        Ok(envelope) => {
            telemetry_context = EventSpineContext::from_envelope(&envelope);
            envelope
        }
        Err(error) => {
            return quarantine(
                store,
                policy,
                &message,
                &telemetry_context,
                QuarantineReason::StoredContractInvalid,
                RelayMessageFailure::StoredContract(error),
            )
            .await;
        }
    };
    let definition = match registry.validate(&envelope) {
        Ok(definition) => *definition,
        Err(error) => {
            return quarantine(
                store,
                policy,
                &message,
                &telemetry_context,
                QuarantineReason::StoredContractInvalid,
                RelayMessageFailure::StoredContract(error),
            )
            .await;
        }
    };

    let publication_started = Instant::now();
    match publisher.publish(definition, &envelope).await {
        Ok(receipt) => {
            let disposition = receipt.disposition();
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Publication,
                match disposition {
                    PublishDisposition::Persisted => EventSpineOutcome::Succeeded,
                    PublishDisposition::Duplicate => EventSpineOutcome::Duplicate,
                },
                message.attempt(),
                publication_started.elapsed(),
            );
            let persistence_started = Instant::now();
            let persistence_result = store
                .mark_published(
                    message.message_source(),
                    message.message_id(),
                    &policy.lease_owner,
                )
                .await
                .map_err(RelayError::from);
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Persistence,
                if persistence_result.is_ok() {
                    EventSpineOutcome::Succeeded
                } else {
                    EventSpineOutcome::Failed
                },
                message.attempt(),
                persistence_started.elapsed(),
            );
            persistence_result?;
            Ok(RelayOutcome::Published {
                disposition,
                attempt: message.attempt(),
            })
        }
        Err(error) => {
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Publication,
                EventSpineOutcome::Failed,
                message.attempt(),
                publication_started.elapsed(),
            );
            resolve_failure(store, policy, &message, &telemetry_context, error).await
        }
    }
}

async fn resolve_failure(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    telemetry_context: &EventSpineContext,
    failure: PublishError,
) -> Result<RelayOutcome, RelayError> {
    let failure_kind = failure.kind();
    match retry_decision(
        policy,
        message.message_source(),
        message.message_id(),
        message.attempt(),
        failure_kind,
    ) {
        RetryDecision::Retry {
            delay,
            failure_code,
        } => {
            let persistence_started = Instant::now();
            let persistence_result = store
                .release_for_retry(
                    message.message_source(),
                    message.message_id(),
                    &policy.lease_owner,
                    delay,
                    failure_code,
                )
                .await
                .map_err(RelayError::from);
            record_event_spine_operation(
                telemetry_context,
                EventSpineStage::Persistence,
                if persistence_result.is_ok() {
                    EventSpineOutcome::RetryScheduled
                } else {
                    EventSpineOutcome::Failed
                },
                message.attempt(),
                persistence_started.elapsed(),
            );
            persistence_result?;
            Ok(RelayOutcome::RetryScheduled {
                failure,
                attempt: message.attempt(),
                delay,
            })
        }
        RetryDecision::Quarantine(reason) => {
            quarantine(
                store,
                policy,
                message,
                telemetry_context,
                reason,
                RelayMessageFailure::Publication(failure),
            )
            .await
        }
    }
}

async fn quarantine(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    telemetry_context: &EventSpineContext,
    reason: QuarantineReason,
    failure: RelayMessageFailure,
) -> Result<RelayOutcome, RelayError> {
    let persistence_started = Instant::now();
    let persistence_result = store
        .quarantine(
            message.message_source(),
            message.message_id(),
            &policy.lease_owner,
            reason.code(),
        )
        .await
        .map_err(RelayError::from);
    record_event_spine_operation(
        telemetry_context,
        EventSpineStage::Persistence,
        if persistence_result.is_ok() {
            EventSpineOutcome::Quarantined
        } else {
            EventSpineOutcome::Failed
        },
        message.attempt(),
        persistence_started.elapsed(),
    );
    persistence_result?;
    Ok(RelayOutcome::Quarantined {
        reason,
        attempt: message.attempt(),
        failure,
    })
}

enum RetryDecision {
    Retry {
        delay: Duration,
        failure_code: &'static str,
    },
    Quarantine(QuarantineReason),
}

fn retry_decision(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
    failure: PublishErrorKind,
) -> RetryDecision {
    match failure {
        PublishErrorKind::Contract => RetryDecision::Quarantine(QuarantineReason::PublishContract),
        PublishErrorKind::Rejected => {
            RetryDecision::Quarantine(QuarantineReason::TransportRejected)
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown
            if attempt >= policy.max_attempts =>
        {
            RetryDecision::Quarantine(match failure {
                PublishErrorKind::Unavailable => QuarantineReason::AttemptsExhaustedUnavailable,
                _ => QuarantineReason::AttemptsExhaustedConfirmationUnknown,
            })
        }
        PublishErrorKind::Unavailable | PublishErrorKind::ConfirmationUnknown => {
            RetryDecision::Retry {
                delay: retry_delay(policy, message_source, message_id, attempt),
                failure_code: match failure {
                    PublishErrorKind::Unavailable => "transport_unavailable",
                    _ => "confirmation_unknown",
                },
            }
        }
    }
}

fn retry_delay(
    policy: &RelayPolicy,
    message_source: &str,
    message_id: &str,
    attempt: u32,
) -> Duration {
    let exponent = attempt.saturating_sub(1).min(63);
    let base_milliseconds = policy.base_retry_delay.as_millis();
    let exponential = base_milliseconds
        .checked_shl(exponent)
        .unwrap_or(u128::MAX)
        .min(policy.max_retry_delay.as_millis());
    let lower = exponential.div_ceil(2);
    let width = exponential.saturating_sub(lower).saturating_add(1);
    let jitter = u128::from(identity_hash(message_source, message_id, attempt)) % width;
    let milliseconds = lower.saturating_add(jitter);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX))
}

fn identity_hash(message_source: &str, message_id: &str, attempt: u32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in message_source
        .bytes()
        .chain([0])
        .chain(message_id.bytes())
        .chain(attempt.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Stable relay-orchestration failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayErrorKind {
    /// Retry or attempt configuration is outside supported bounds.
    InvalidPolicy,
    /// The outbox store was unavailable or could not commit an operation.
    Storage,
    /// The outbox rejected a state transition or returned invalid state.
    Outbox,
}

impl Display for RelayErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("outbox relay policy is invalid"),
            Self::Storage => formatter.write_str("outbox relay storage operation failed"),
            Self::Outbox => formatter.write_str("outbox relay state transition failed"),
        }
    }
}

/// Relay failure with bounded public text and a preserved internal cause.
#[derive(Debug)]
pub struct RelayError {
    kind: RelayErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl RelayError {
    /// Return the stable category used by worker health and restart policy.
    #[must_use]
    pub const fn kind(&self) -> RelayErrorKind {
        self.kind
    }

    const fn invalid_policy(reason: &'static str) -> Self {
        Self {
            kind: RelayErrorKind::InvalidPolicy,
            reason: Some(reason),
            source: None,
        }
    }
}

impl Display for RelayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for RelayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<OutboxStoreError> for RelayError {
    fn from(error: OutboxStoreError) -> Self {
        Self {
            kind: match error.kind() {
                OutboxStoreErrorKind::Unavailable => RelayErrorKind::Storage,
                OutboxStoreErrorKind::StateTransition | OutboxStoreErrorKind::Invariant => {
                    RelayErrorKind::Outbox
                }
            },
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QuarantineReason, RelayErrorKind, RelayOutcome, RelayPolicy, RetryDecision, relay_once,
        retry_decision, retry_delay,
    };
    use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRegistry};
    use edgeagent_messaging::{
        ClaimedMessage, MessagePublisher, OutboxRelayStore, OutboxStoreFuture, PublishDisposition,
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

    #[derive(Default)]
    struct InMemoryStore {
        claimed: Option<ClaimedMessage>,
        transitions: Vec<&'static str>,
    }

    impl OutboxRelayStore for InMemoryStore {
        fn claim_one<'operation>(
            &'operation mut self,
            _lease_owner: &'operation str,
            _lease_duration: Duration,
        ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
            Box::pin(async move {
                self.transitions.push("claimed");
                Ok(self.claimed.take())
            })
        }

        fn mark_published<'operation>(
            &'operation mut self,
            _message_source: &'operation str,
            _message_id: &'operation str,
            _lease_owner: &'operation str,
        ) -> OutboxStoreFuture<'operation, ()> {
            Box::pin(async move {
                self.transitions.push("published");
                Ok(())
            })
        }

        fn release_for_retry<'operation>(
            &'operation mut self,
            _message_source: &'operation str,
            _message_id: &'operation str,
            _lease_owner: &'operation str,
            _retry_after: Duration,
            _failure_code: &'operation str,
        ) -> OutboxStoreFuture<'operation, ()> {
            Box::pin(async move {
                self.transitions.push("retry_scheduled");
                Ok(())
            })
        }

        fn quarantine<'operation>(
            &'operation mut self,
            _message_source: &'operation str,
            _message_id: &'operation str,
            _lease_owner: &'operation str,
            _reason: &'operation str,
        ) -> OutboxStoreFuture<'operation, ()> {
            Box::pin(async move {
                self.transitions.push("quarantined");
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

    fn claimed_message() -> Result<ClaimedMessage, Box<dyn Error>> {
        let envelope = COMMAND.build(
            MessageMetadata {
                id: "relay-port-message-01".to_owned(),
                source: Component::Gateway.source_uri().to_owned(),
                message_type: COMMAND.message_type.to_owned(),
                subject: "order/relay-port-order-01".to_owned(),
                time: "2026-09-28T00:00:00Z".to_owned(),
                data_schema: COMMAND.data_schema.to_owned(),
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
        assert_eq!(store.transitions, ["claimed", "published"]);
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
}
