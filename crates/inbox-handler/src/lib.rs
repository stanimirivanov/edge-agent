//! One-delivery transactional consumer orchestration.

#![forbid(unsafe_code)]

use edgeagent_contracts::{
    MessageContractError, MessageEnvelope, MessageRegistry, MessageRoutingError,
};
use edgeagent_inbox_postgres::{
    DeliveryDisposition as InboxDisposition, InboxError, InboxErrorKind, PostgresInbox,
    QuarantineDisposition, QuarantineEvidence,
};
use edgeagent_messaging::{
    ConsumeError, DeliveryDisposition as SettlementDisposition, MessageDelivery,
};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio_postgres::{Client, Transaction};

const MAX_ATTEMPTS: u32 = 100;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Future returned by one transactional domain handler invocation.
pub type HandlerFuture<'handler> =
    Pin<Box<dyn Future<Output = Result<(), HandlerFailure>> + Send + 'handler>>;

/// Service-owned domain work performed inside the inbox transaction.
pub trait TransactionalMessageHandler: Sync {
    /// Apply the first delivery's domain transition and enqueue resulting messages.
    ///
    /// Implementations MUST restrict this future to database work on the supplied
    /// transaction. Network calls and other external effects cannot be rolled back.
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler Transaction<'_>,
        envelope: &'handler MessageEnvelope,
    ) -> HandlerFuture<'handler>;
}

/// Handler failure classification used by retry and quarantine policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerFailureKind {
    /// The same immutable message may succeed on a later delivery.
    Transient,
    /// Retrying cannot change the result without code, policy, or operator action.
    Permanent,
}

impl Display for HandlerFailureKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient => formatter.write_str("message handler failed transiently"),
            Self::Permanent => formatter.write_str("message handler rejected the delivery"),
        }
    }
}

/// Bounded handler failure with an operator-safe static reason code.
#[derive(Debug)]
pub struct HandlerFailure {
    kind: HandlerFailureKind,
    code: &'static str,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl HandlerFailure {
    /// Construct a transient failure without exposing internal details.
    #[must_use]
    pub const fn transient(code: &'static str) -> Self {
        Self {
            kind: HandlerFailureKind::Transient,
            code,
            source: None,
        }
    }

    /// Construct a permanent failure without exposing internal details.
    #[must_use]
    pub const fn permanent(code: &'static str) -> Self {
        Self {
            kind: HandlerFailureKind::Permanent,
            code,
            source: None,
        }
    }

    /// Construct a classified failure while preserving its internal cause.
    #[must_use]
    pub fn with_source<E>(kind: HandlerFailureKind, code: &'static str, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            code,
            source: Some(Box::new(source)),
        }
    }

    /// Return the retry classification.
    #[must_use]
    pub const fn kind(&self) -> HandlerFailureKind {
        self.kind
    }

    /// Return the candidate quarantine reason code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl Display for HandlerFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for HandlerFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

/// Bounded retry policy shared by every delivery handled by one logical consumer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandlerPolicy {
    consumer_name: String,
    max_delivery_attempts: u32,
    base_retry_delay: Duration,
    max_retry_delay: Duration,
}

impl HandlerPolicy {
    /// Construct a validated consumer policy.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPolicy` for an unstable consumer name or unsafe retry bounds.
    pub fn new(
        consumer_name: impl Into<String>,
        max_delivery_attempts: u32,
        base_retry_delay: Duration,
        max_retry_delay: Duration,
    ) -> Result<Self, HandlerError> {
        let consumer_name = consumer_name.into();
        if consumer_name.is_empty()
            || consumer_name.len() > 128
            || !consumer_name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(HandlerError::invalid_policy(
                "consumer_name must be a lowercase ASCII token of 1 to 128 bytes",
            ));
        }
        if max_delivery_attempts == 0 || max_delivery_attempts > MAX_ATTEMPTS {
            return Err(HandlerError::invalid_policy(
                "max_delivery_attempts must be between 1 and 100",
            ));
        }
        if base_retry_delay < Duration::from_millis(1) {
            return Err(HandlerError::invalid_policy(
                "base_retry_delay must be at least 1 millisecond",
            ));
        }
        if max_retry_delay < base_retry_delay || max_retry_delay > MAX_RETRY_DELAY {
            return Err(HandlerError::invalid_policy(
                "max_retry_delay must be at least the base delay and at most 24 hours",
            ));
        }
        Ok(Self {
            consumer_name,
            max_delivery_attempts,
            base_retry_delay,
            max_retry_delay,
        })
    }
}

/// Classified message-processing failure retained in an observable outcome.
#[derive(Debug)]
pub enum MessageFailure {
    /// Structured CloudEvents bytes could not be decoded or validated.
    Envelope(MessageContractError),
    /// The decoded envelope is unsupported or violates its registered route.
    Routing(MessageRoutingError),
    /// Inbox state rejected or could not persist the delivery.
    Inbox(InboxError),
    /// PostgreSQL could not begin, roll back, or commit a transaction.
    Storage(tokio_postgres::Error),
    /// The service-owned domain handler returned a classified failure.
    Handler(HandlerFailure),
}

impl Display for MessageFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope(_) => formatter.write_str("message envelope is invalid"),
            Self::Routing(_) => formatter.write_str("message route is invalid"),
            Self::Inbox(error) => Display::fmt(error, formatter),
            Self::Storage(_) => formatter.write_str("message transaction failed"),
            Self::Handler(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MessageFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Envelope(error) => Some(error),
            Self::Routing(error) => Some(error),
            Self::Inbox(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Handler(error) => Some(error),
        }
    }
}

/// Confirmed result of processing one transport delivery.
#[derive(Debug)]
pub enum HandlingOutcome {
    /// A first delivery committed its domain transition and was acknowledged.
    Applied {
        /// One-based transport delivery attempt.
        attempt: u32,
    },
    /// A committed inbox identity suppressed duplicate domain work and was acknowledged.
    Duplicate {
        /// One-based transport delivery attempt.
        attempt: u32,
    },
    /// A transient failure was negatively acknowledged with a bounded delay.
    RetryRequested {
        /// One-based transport delivery attempt.
        attempt: u32,
        /// Deterministic retry delay confirmed by the broker.
        delay: Duration,
        /// Failure that caused redelivery.
        failure: MessageFailure,
    },
    /// Permanent evidence committed and terminal broker settlement was confirmed.
    Quarantined {
        /// One-based transport delivery attempt.
        attempt: u32,
        /// Whether evidence was inserted or already present after redelivery.
        disposition: QuarantineDisposition,
        /// Stable reason retained with the exact payload.
        failure_code: &'static str,
        /// Failure that caused terminal handling.
        failure: MessageFailure,
    },
}

/// Process and settle exactly one delivery.
///
/// Domain writes occur only for a first inbox delivery and share its transaction.
/// Acknowledgement follows commit. Permanent failures commit quarantine evidence
/// before terminal settlement. Transient failures request deterministic delayed
/// redelivery; infrastructure failures are never converted into terminal success.
///
/// # Errors
///
/// Returns a configuration, invariant, quarantine, or broker-settlement failure.
/// The owned delivery is dropped without successful settlement on error.
pub async fn handle_once(
    client: &mut Client,
    registry: &MessageRegistry<'_>,
    handler: &dyn TransactionalMessageHandler,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
) -> Result<HandlingOutcome, HandlerError> {
    let envelope = match MessageEnvelope::from_json(delivery.payload()) {
        Ok(envelope) => envelope,
        Err(error) => {
            return quarantine(
                client,
                policy,
                delivery,
                "envelope_invalid",
                MessageFailure::Envelope(error),
            )
            .await;
        }
    };
    if let Err(error) = registry.validate(&envelope) {
        return quarantine(
            client,
            policy,
            delivery,
            "routing_invalid",
            MessageFailure::Routing(error),
        )
        .await;
    }

    let inbox = PostgresInbox;
    let transaction = match client.transaction().await {
        Ok(transaction) => transaction,
        Err(error) => {
            return retry(delivery, policy, MessageFailure::Storage(error)).await;
        }
    };
    let inbox_disposition = match inbox
        .record_delivery(&transaction, &policy.consumer_name, registry, &envelope)
        .await
    {
        Ok(disposition) => disposition,
        Err(error) => {
            let kind = error.kind();
            if let Err(rollback_error) = transaction.rollback().await {
                return retry(delivery, policy, MessageFailure::Storage(rollback_error)).await;
            }
            return match kind {
                InboxErrorKind::Contract => {
                    quarantine(
                        client,
                        policy,
                        delivery,
                        "routing_invalid",
                        MessageFailure::Inbox(error),
                    )
                    .await
                }
                InboxErrorKind::MessageIdentityConflict => {
                    quarantine(
                        client,
                        policy,
                        delivery,
                        "message_identity_conflict",
                        MessageFailure::Inbox(error),
                    )
                    .await
                }
                InboxErrorKind::Storage => {
                    retry(delivery, policy, MessageFailure::Inbox(error)).await
                }
                InboxErrorKind::InvalidConsumerName
                | InboxErrorKind::InvalidQuarantineEvidence
                | InboxErrorKind::QuarantineIdentityConflict
                | InboxErrorKind::StorageInvariant => Err(HandlerError::inbox(error)),
            };
        }
    };

    match inbox_disposition {
        InboxDisposition::Duplicate => {
            if let Err(error) = transaction.commit().await {
                return retry(delivery, policy, MessageFailure::Storage(error)).await;
            }
            acknowledge(delivery, HandlingOutcomeKind::Duplicate).await
        }
        InboxDisposition::FirstDelivery => match handler.handle(&transaction, &envelope).await {
            Ok(()) => {
                if let Err(error) = transaction.commit().await {
                    return retry(delivery, policy, MessageFailure::Storage(error)).await;
                }
                acknowledge(delivery, HandlingOutcomeKind::Applied).await
            }
            Err(failure) => {
                if let Err(rollback_error) = transaction.rollback().await {
                    return retry(delivery, policy, MessageFailure::Storage(rollback_error)).await;
                }
                resolve_handler_failure(client, policy, delivery, failure).await
            }
        },
    }
}

#[derive(Clone, Copy)]
enum HandlingOutcomeKind {
    Applied,
    Duplicate,
}

async fn acknowledge(
    delivery: MessageDelivery,
    outcome: HandlingOutcomeKind,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    delivery
        .settle(SettlementDisposition::Acknowledge)
        .await
        .map_err(HandlerError::settlement)?;
    Ok(match outcome {
        HandlingOutcomeKind::Applied => HandlingOutcome::Applied { attempt },
        HandlingOutcomeKind::Duplicate => HandlingOutcome::Duplicate { attempt },
    })
}

async fn resolve_handler_failure(
    client: &mut Client,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    failure: HandlerFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure.code())?;
    if should_retry_handler(
        policy,
        delivery.metadata().delivery_attempt(),
        failure.kind(),
    ) {
        retry(delivery, policy, MessageFailure::Handler(failure)).await
    } else {
        let code = failure.code();
        quarantine(
            client,
            policy,
            delivery,
            code,
            MessageFailure::Handler(failure),
        )
        .await
    }
}

fn should_retry_handler(policy: &HandlerPolicy, attempt: u32, kind: HandlerFailureKind) -> bool {
    kind == HandlerFailureKind::Transient && attempt < policy.max_delivery_attempts
}

async fn retry(
    delivery: MessageDelivery,
    policy: &HandlerPolicy,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    let delay = retry_delay(policy, delivery.metadata().message_key(), attempt);
    delivery
        .settle(SettlementDisposition::RetryAfter(delay))
        .await
        .map_err(HandlerError::settlement)?;
    Ok(HandlingOutcome::RetryRequested {
        attempt,
        delay,
        failure,
    })
}

async fn quarantine(
    client: &mut Client,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    failure_code: &'static str,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure_code)?;
    let evidence = QuarantineEvidence::new(
        delivery.metadata().message_key(),
        delivery.metadata().subject(),
        delivery.metadata().delivery_attempt(),
        delivery.payload(),
        failure_code,
    )
    .map_err(HandlerError::inbox)?;
    let inbox = PostgresInbox;
    let transaction = match client.transaction().await {
        Ok(transaction) => transaction,
        Err(error) => {
            return retry(delivery, policy, MessageFailure::Storage(error)).await;
        }
    };
    let disposition = match inbox
        .quarantine_delivery(&transaction, &policy.consumer_name, evidence)
        .await
    {
        Ok(disposition) => disposition,
        Err(error) if error.kind() == InboxErrorKind::Storage => {
            if let Err(rollback_error) = transaction.rollback().await {
                return retry(delivery, policy, MessageFailure::Storage(rollback_error)).await;
            }
            return retry(delivery, policy, MessageFailure::Inbox(error)).await;
        }
        Err(error) => return Err(HandlerError::inbox(error)),
    };
    if let Err(error) = transaction.commit().await {
        return retry(delivery, policy, MessageFailure::Storage(error)).await;
    }
    let attempt = delivery.metadata().delivery_attempt();
    delivery
        .settle(SettlementDisposition::Quarantined)
        .await
        .map_err(HandlerError::settlement)?;
    Ok(HandlingOutcome::Quarantined {
        attempt,
        disposition,
        failure_code,
        failure,
    })
}

fn validate_failure_code(code: &str) -> Result<(), HandlerError> {
    if code.is_empty()
        || code.len() > 64
        || !code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Err(HandlerError::invalid_handler_failure(
            "handler failure code must be a lowercase ASCII token of 1 to 64 bytes",
        ))
    } else {
        Ok(())
    }
}

fn retry_delay(policy: &HandlerPolicy, message_key: &str, attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(63);
    let base_milliseconds = policy.base_retry_delay.as_millis();
    let exponential = base_milliseconds
        .checked_shl(exponent)
        .unwrap_or(u128::MAX)
        .min(policy.max_retry_delay.as_millis());
    let lower = exponential.div_ceil(2);
    let width = exponential.saturating_sub(lower).saturating_add(1);
    let jitter = u128::from(identity_hash(message_key, attempt)) % width;
    let milliseconds = lower.saturating_add(jitter);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX))
}

fn identity_hash(message_key: &str, attempt: u32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in message_key.bytes().chain([0]).chain(attempt.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Stable consumer-orchestration failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerErrorKind {
    /// Consumer identity or retry configuration is invalid.
    InvalidPolicy,
    /// A domain handler returned an unsafe reason code.
    InvalidHandlerFailure,
    /// Inbox state violates a non-retriable invariant.
    Inbox,
    /// Broker settlement failed or its confirmation is unknown.
    Settlement,
}

impl Display for HandlerErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("message handler policy is invalid"),
            Self::InvalidHandlerFailure => {
                formatter.write_str("message handler failure classification is invalid")
            }
            Self::Inbox => formatter.write_str("message handler inbox transition failed"),
            Self::Settlement => formatter.write_str("message delivery settlement failed"),
        }
    }
}

/// Coordinator failure with bounded public text and a preserved internal cause.
#[derive(Debug)]
pub struct HandlerError {
    kind: HandlerErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl HandlerError {
    /// Return the stable category used by worker health and restart policy.
    #[must_use]
    pub const fn kind(&self) -> HandlerErrorKind {
        self.kind
    }

    const fn invalid_policy(reason: &'static str) -> Self {
        Self {
            kind: HandlerErrorKind::InvalidPolicy,
            reason: Some(reason),
            source: None,
        }
    }

    const fn invalid_handler_failure(reason: &'static str) -> Self {
        Self {
            kind: HandlerErrorKind::InvalidHandlerFailure,
            reason: Some(reason),
            source: None,
        }
    }

    fn inbox(error: InboxError) -> Self {
        Self {
            kind: HandlerErrorKind::Inbox,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    fn settlement(error: ConsumeError) -> Self {
        Self {
            kind: HandlerErrorKind::Settlement,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

impl Display for HandlerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for HandlerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HandlerErrorKind, HandlerFailure, HandlerFailureKind, HandlerPolicy, retry_delay,
        should_retry_handler, validate_failure_code,
    };
    use std::time::Duration;

    #[test]
    fn policy_and_failure_codes_are_bounded() {
        assert_eq!(
            HandlerPolicy::new(
                "Execution Simulator",
                3,
                Duration::from_secs(1),
                Duration::from_secs(30),
            )
            .err()
            .map(|error| error.kind()),
            Some(HandlerErrorKind::InvalidPolicy)
        );
        assert_eq!(
            HandlerPolicy::new(
                "execution_simulator_v1",
                0,
                Duration::from_secs(1),
                Duration::from_secs(30),
            )
            .err()
            .map(|error| error.kind()),
            Some(HandlerErrorKind::InvalidPolicy)
        );
        assert_eq!(
            validate_failure_code(HandlerFailure::permanent("Unsafe Reason").code())
                .err()
                .map(|error| error.kind()),
            Some(HandlerErrorKind::InvalidHandlerFailure)
        );
    }

    #[test]
    fn retry_delay_is_deterministic_bounded_and_identity_jittered() -> Result<(), HandlerErrorKind>
    {
        let policy = HandlerPolicy::new(
            "execution_simulator_v1",
            4,
            Duration::from_secs(2),
            Duration::from_secs(10),
        )
        .map_err(|error| error.kind())?;
        let first = retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 1);
        assert_eq!(first, retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 1));
        assert!(first >= Duration::from_secs(1));
        assert!(first <= Duration::from_secs(2));
        assert_ne!(first, retry_delay(&policy, "18:EDGEAGENT_COMMANDS:8", 1));
        assert!(retry_delay(&policy, "18:EDGEAGENT_COMMANDS:7", 100) <= Duration::from_secs(10));
        assert!(should_retry_handler(
            &policy,
            3,
            HandlerFailureKind::Transient
        ));
        assert!(!should_retry_handler(
            &policy,
            4,
            HandlerFailureKind::Transient
        ));
        assert!(!should_retry_handler(
            &policy,
            1,
            HandlerFailureKind::Permanent
        ));
        Ok(())
    }
}
