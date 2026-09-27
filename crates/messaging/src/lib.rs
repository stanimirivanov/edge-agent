//! Application-owned messaging ports and portable publication outcomes.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Owned future returned by a message publisher implementation.
pub type PublishFuture<'publisher> =
    Pin<Box<dyn Future<Output = Result<PublishReceipt, PublishError>> + Send + 'publisher>>;

/// Application-owned boundary for durable message publication.
///
/// Implementations MUST derive the transport destination from `definition`,
/// validate `envelope` against that definition, and complete only after the
/// durable transport confirms acceptance or reports a classified failure.
pub trait MessagePublisher: Send + Sync {
    /// Publish one validated envelope using its registered routing definition.
    ///
    /// A caller retrying an unavailable or ambiguous publication MUST preserve
    /// the envelope identity and content. A successful return confirms broker
    /// persistence, not exactly-once domain processing.
    fn publish<'publisher>(
        &'publisher self,
        definition: MessageDefinition,
        envelope: &'publisher MessageEnvelope,
    ) -> PublishFuture<'publisher>;
}

/// Broker-confirmed result of a durable publication attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishReceipt {
    disposition: PublishDisposition,
}

impl PublishReceipt {
    /// Construct a portable receipt from a broker acknowledgement.
    #[must_use]
    pub const fn new(disposition: PublishDisposition) -> Self {
        Self { disposition }
    }

    /// Return whether the broker persisted this attempt or recognized a retry.
    #[must_use]
    pub const fn disposition(self) -> PublishDisposition {
        self.disposition
    }
}

/// Portable disposition reported after durable broker acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishDisposition {
    /// The broker persisted this message as a new entry.
    Persisted,
    /// The broker recognized the stable message identity inside its deduplication window.
    Duplicate,
}

/// Caller-actionable publication failure category shared by broker adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishErrorKind {
    /// The definition or envelope violates the application message contract.
    Contract,
    /// No publish request was accepted; retry the same envelope after backoff.
    Unavailable,
    /// The broker explicitly rejected the request; retry only after remediation.
    Rejected,
    /// The request may have persisted but acknowledgement was lost or unreadable.
    ConfirmationUnknown,
}

impl Display for PublishErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("message contract rejected publication"),
            Self::Unavailable => formatter.write_str("message transport is unavailable"),
            Self::Rejected => formatter.write_str("message transport rejected publication"),
            Self::ConfirmationUnknown => {
                formatter.write_str("message publication confirmation is unknown")
            }
        }
    }
}

/// Publication failure with a stable public category and preserved internal cause.
#[derive(Debug)]
pub struct PublishError {
    kind: PublishErrorKind,
    source: Box<dyn Error + Send + Sync>,
}

impl PublishError {
    /// Wrap an adapter or contract cause without exposing it through `Display`.
    #[must_use]
    pub fn with_source<E>(kind: PublishErrorKind, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            source: Box::new(source),
        }
    }

    /// Return the stable category that controls retry and operator behavior.
    #[must_use]
    pub const fn kind(&self) -> PublishErrorKind {
        self.kind
    }
}

impl Display for PublishError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for PublishError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<MessageRoutingError> for PublishError {
    fn from(error: MessageRoutingError) -> Self {
        Self::with_source(PublishErrorKind::Contract, error)
    }
}

/// Future returned while waiting for one durable consumer delivery.
pub type ReceiveFuture<'consumer> =
    Pin<Box<dyn Future<Output = Result<MessageDelivery, ConsumeError>> + Send + 'consumer>>;

/// Future returned while confirming one delivery settlement with the broker.
pub type SettlementFuture =
    Pin<Box<dyn Future<Output = Result<(), ConsumeError>> + Send + 'static>>;

/// Application-owned boundary for one-at-a-time durable message consumption.
pub trait MessageConsumer: Send {
    /// Wait for the next delivery.
    ///
    /// Cancellation drops the receive future without acknowledging a message.
    /// The returned delivery remains unsettled until the caller explicitly
    /// chooses one terminal disposition.
    fn receive(&mut self) -> ReceiveFuture<'_>;
}

/// One-shot broker settlement owned by a received delivery.
pub trait DeliverySettlement: Send {
    /// Apply and confirm exactly one terminal disposition.
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture;
}

/// Portable metadata needed for idempotency, ordering diagnostics, and backpressure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryMetadata {
    subject: String,
    delivery_attempt: u32,
    pending: u64,
    stream_sequence: u64,
    consumer_sequence: u64,
}

impl DeliveryMetadata {
    /// Construct validated transport metadata.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` when the subject is empty or the broker reports a
    /// zero delivery attempt.
    pub fn new(
        subject: impl Into<String>,
        delivery_attempt: u32,
        pending: u64,
        stream_sequence: u64,
        consumer_sequence: u64,
    ) -> Result<Self, ConsumeError> {
        let subject = subject.into();
        if subject.is_empty() {
            return Err(ConsumeError::protocol("delivery subject must not be empty"));
        }
        if delivery_attempt == 0 {
            return Err(ConsumeError::protocol(
                "delivery attempt must be greater than zero",
            ));
        }
        Ok(Self {
            subject,
            delivery_attempt,
            pending,
            stream_sequence,
            consumer_sequence,
        })
    }

    /// Return the broker subject that selected this delivery.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Return the one-based broker delivery attempt.
    #[must_use]
    pub const fn delivery_attempt(&self) -> u32 {
        self.delivery_attempt
    }

    /// Return messages known by the broker to be pending for this consumer.
    #[must_use]
    pub const fn pending(&self) -> u64 {
        self.pending
    }

    /// Return the broker stream sequence used for ordering diagnostics.
    #[must_use]
    pub const fn stream_sequence(&self) -> u64 {
        self.stream_sequence
    }

    /// Return the consumer delivery sequence used for redelivery diagnostics.
    #[must_use]
    pub const fn consumer_sequence(&self) -> u64 {
        self.consumer_sequence
    }
}

/// An untrusted delivery whose settlement is consumed exactly once.
pub struct MessageDelivery {
    payload: Vec<u8>,
    metadata: DeliveryMetadata,
    settlement: Box<dyn DeliverySettlement>,
}

impl MessageDelivery {
    /// Construct a delivery from a transport adapter.
    #[must_use]
    pub fn new(
        payload: Vec<u8>,
        metadata: DeliveryMetadata,
        settlement: Box<dyn DeliverySettlement>,
    ) -> Self {
        Self {
            payload,
            metadata,
            settlement,
        }
    }

    /// Return the untrusted structured envelope bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Return portable broker delivery metadata.
    #[must_use]
    pub const fn metadata(&self) -> &DeliveryMetadata {
        &self.metadata
    }

    /// Consume this delivery and confirm its terminal broker disposition.
    pub fn settle(self, disposition: DeliveryDisposition) -> SettlementFuture {
        self.settlement.settle(disposition)
    }
}

impl std::fmt::Debug for MessageDelivery {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MessageDelivery")
            .field("payload_bytes", &self.payload.len())
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

/// Terminal action applied after a handler commits or classifies a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// Confirm successful handling after the local transaction commits.
    Acknowledge,
    /// Request redelivery after a bounded delay while preserving message identity.
    RetryAfter(Duration),
    /// Stop redelivery only after durable quarantine evidence has committed.
    Quarantined,
}

/// Caller-actionable delivery and settlement failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsumeErrorKind {
    /// The consumer stream is unavailable; retry receive under service policy.
    Unavailable,
    /// Broker delivery metadata violates the portable contract.
    Protocol,
    /// The requested settlement is outside portable bounds.
    InvalidDisposition,
    /// Settlement may have reached the broker, but confirmation was lost.
    ConfirmationUnknown,
}

impl Display for ConsumeErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("message consumer is unavailable"),
            Self::Protocol => formatter.write_str("message delivery protocol is invalid"),
            Self::InvalidDisposition => formatter.write_str("message settlement is invalid"),
            Self::ConfirmationUnknown => {
                formatter.write_str("message settlement confirmation is unknown")
            }
        }
    }
}

/// Consumer failure with stable public text and a preserved internal cause.
#[derive(Debug)]
pub struct ConsumeError {
    kind: ConsumeErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ConsumeError {
    /// Wrap an adapter cause without exposing it through `Display`.
    #[must_use]
    pub fn with_source<E>(kind: ConsumeErrorKind, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            reason: None,
            source: Some(Box::new(source)),
        }
    }

    /// Preserve an adapter error that already uses a boxed dynamic source.
    #[must_use]
    pub fn with_boxed_source(kind: ConsumeErrorKind, source: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            kind,
            reason: None,
            source: Some(source),
        }
    }

    /// Construct a bounded invalid-disposition error.
    #[must_use]
    pub const fn invalid_disposition(reason: &'static str) -> Self {
        Self {
            kind: ConsumeErrorKind::InvalidDisposition,
            reason: Some(reason),
            source: None,
        }
    }

    /// Construct a bounded unavailable-stream error without an internal cause.
    #[must_use]
    pub const fn unavailable(reason: &'static str) -> Self {
        Self {
            kind: ConsumeErrorKind::Unavailable,
            reason: Some(reason),
            source: None,
        }
    }

    /// Construct a bounded protocol-contract error without an internal cause.
    #[must_use]
    pub const fn protocol(reason: &'static str) -> Self {
        Self {
            kind: ConsumeErrorKind::Protocol,
            reason: Some(reason),
            source: None,
        }
    }

    /// Return the stable category used by retry and health policy.
    #[must_use]
    pub const fn kind(&self) -> ConsumeErrorKind {
        self.kind
    }
}

impl Display for ConsumeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for ConsumeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsumeErrorKind, DeliveryMetadata, PublishError, PublishErrorKind};
    use edgeagent_contracts::MessageRoutingError;
    use std::error::Error;

    #[test]
    fn public_error_is_stable_while_contract_cause_is_preserved() {
        let error = PublishError::from(MessageRoutingError::UnsupportedMessageType(
            "com.edgeagent.research.unknown.v1".to_owned(),
        ));

        assert_eq!(error.kind(), PublishErrorKind::Contract);
        assert_eq!(error.to_string(), "message contract rejected publication");
        assert!(
            error
                .source()
                .is_some_and(|source| source.to_string().contains("unsupported message type"))
        );
    }

    #[test]
    fn delivery_metadata_requires_a_subject_and_positive_attempt() {
        assert_eq!(
            DeliveryMetadata::new("", 1, 0, 1, 1)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            DeliveryMetadata::new("edgeagent.command.execution.submit.v1", 0, 0, 1, 1)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        let metadata = DeliveryMetadata::new("edgeagent.command.execution.submit.v1", 2, 7, 11, 13);
        assert_eq!(
            metadata
                .map(|metadata| (metadata.delivery_attempt(), metadata.pending()))
                .map_err(|error| error.kind()),
            Ok((2, 7))
        );
    }
}
