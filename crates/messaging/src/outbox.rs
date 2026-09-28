//! Application-owned ports and values for durable outbound message relay.

use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Boxed future returned by an outbound relay storage port.
pub type OutboxStoreFuture<'operation, Output> =
    Pin<Box<dyn Future<Output = Result<Output, OutboxStoreError>> + Send + 'operation>>;

/// Persistence capabilities required by one outbound relay iteration.
///
/// Implementations own their transaction boundaries. A claim must be committed
/// before it is returned so publication never occurs inside a storage transaction.
pub trait OutboxRelayStore: Send {
    /// Claim at most one available message for this relay worker.
    fn claim_one<'operation>(
        &'operation mut self,
        lease_owner: &'operation str,
        lease_duration: Duration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>>;

    /// Record confirmed durable publication under the original lease.
    fn mark_published<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()>;

    /// Release a leased message for a bounded delayed retry.
    fn release_for_retry<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
        retry_after: Duration,
        failure_code: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()>;

    /// Move a leased message into terminal quarantine.
    fn quarantine<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
        reason: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()>;
}

/// One durable outbound message claimed for publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedMessage {
    message_source: String,
    message_id: String,
    message_type: String,
    transport_subject: String,
    envelope: Vec<u8>,
    attempt: u32,
}

impl ClaimedMessage {
    /// Construct a claimed message from a persistence adapter.
    ///
    /// Stored contract fields remain untrusted and are validated before
    /// publication. Only the storage invariant that attempts are one-based is
    /// enforced here.
    ///
    /// # Errors
    ///
    /// Returns `Invariant` when `attempt` is zero.
    pub fn new(
        message_source: String,
        message_id: String,
        message_type: String,
        transport_subject: String,
        envelope: Vec<u8>,
        attempt: u32,
    ) -> Result<Self, OutboxStoreError> {
        if attempt == 0 {
            return Err(OutboxStoreError::invariant(
                "claimed message attempt must be greater than zero",
            ));
        }
        Ok(Self {
            message_source,
            message_id,
            message_type,
            transport_subject,
            envelope,
            attempt,
        })
    }

    /// Return the CloudEvents source identity.
    #[must_use]
    pub fn message_source(&self) -> &str {
        &self.message_source
    }

    /// Return the CloudEvents message identity within the source.
    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// Return the versioned CloudEvents message type.
    #[must_use]
    pub fn message_type(&self) -> &str {
        &self.message_type
    }

    /// Return the derived transport subject retained with the message.
    #[must_use]
    pub fn transport_subject(&self) -> &str {
        &self.transport_subject
    }

    /// Return the exact validated envelope bytes retained by the producer.
    #[must_use]
    pub fn envelope_bytes(&self) -> &[u8] {
        &self.envelope
    }

    /// Return the one-based number of times this record has been claimed.
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Decode and revalidate the stored bytes and denormalized routing metadata.
    ///
    /// # Errors
    ///
    /// Returns a contract error when storage is corrupt or the process registry
    /// no longer supports the exact message major version.
    pub fn validated_envelope(
        &self,
        registry: &MessageRegistry<'_>,
    ) -> Result<MessageEnvelope, MessageRoutingError> {
        let envelope = MessageEnvelope::from_json(&self.envelope)?;
        let definition = registry.validate(&envelope)?;
        require_equal("id", &self.message_id, envelope.id())?;
        require_equal("source", &self.message_source, envelope.source())?;
        require_equal("type", &self.message_type, envelope.message_type())?;
        require_equal(
            "transport_subject",
            &self.transport_subject,
            &definition.subject()?,
        )?;
        Ok(envelope)
    }
}

/// Stable categories exposed by an outbound relay storage adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxStoreErrorKind {
    /// Storage was unavailable or could not commit the operation.
    Unavailable,
    /// A claim or leased state transition was rejected.
    StateTransition,
    /// Returned storage state violated the port contract.
    Invariant,
}

impl Display for OutboxStoreErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("outbox storage is unavailable"),
            Self::StateTransition => formatter.write_str("outbox state transition failed"),
            Self::Invariant => formatter.write_str("outbox storage invariant failed"),
        }
    }
}

/// Outbound relay storage failure with bounded text and an internal cause.
#[derive(Debug)]
pub struct OutboxStoreError {
    kind: OutboxStoreErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl OutboxStoreError {
    /// Construct a classified adapter failure while preserving its cause.
    pub fn with_source<E>(kind: OutboxStoreErrorKind, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            reason: None,
            source: Some(Box::new(source)),
        }
    }

    /// Return the stable category used by relay health policy.
    #[must_use]
    pub const fn kind(&self) -> OutboxStoreErrorKind {
        self.kind
    }

    const fn invariant(reason: &'static str) -> Self {
        Self {
            kind: OutboxStoreErrorKind::Invariant,
            reason: Some(reason),
            source: None,
        }
    }
}

impl Display for OutboxStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for OutboxStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

fn require_equal(
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), MessageRoutingError> {
    if expected == actual {
        Ok(())
    } else {
        Err(MessageRoutingError::ContractMismatch {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimedMessage, OutboxStoreErrorKind};

    #[test]
    fn claimed_message_attempt_is_one_based() {
        let result = ClaimedMessage::new(
            "urn:edgeagent:component:gateway".to_owned(),
            "message-01".to_owned(),
            "com.edgeagent.execution.submit-dry-run-order.v1".to_owned(),
            "edgeagent.command.execution.submit-dry-run-order.v1".to_owned(),
            b"{}".to_vec(),
            0,
        );

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(OutboxStoreErrorKind::Invariant)
        );
    }
}
