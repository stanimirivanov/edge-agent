//! Portable one-at-a-time delivery, metadata, and settlement contracts.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

const MAX_DELIVERY_TEXT_BYTES: usize = 512;

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
    message_key: String,
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
    /// zero delivery attempt or sequence.
    pub fn new(
        message_key: impl Into<String>,
        subject: impl Into<String>,
        delivery_attempt: u32,
        pending: u64,
        stream_sequence: u64,
        consumer_sequence: u64,
    ) -> Result<Self, ConsumeError> {
        let message_key = message_key.into();
        let subject = subject.into();
        validate_delivery_text(
            &message_key,
            "message key must contain 1 to 512 visible ASCII bytes",
        )?;
        validate_delivery_text(
            &subject,
            "delivery subject must contain 1 to 512 visible ASCII bytes",
        )?;
        if delivery_attempt == 0 {
            return Err(ConsumeError::protocol(
                "delivery attempt must be greater than zero",
            ));
        }
        if stream_sequence == 0 || consumer_sequence == 0 {
            return Err(ConsumeError::protocol(
                "delivery sequences must be greater than zero",
            ));
        }
        Ok(Self {
            message_key,
            subject,
            delivery_attempt,
            pending,
            stream_sequence,
            consumer_sequence,
        })
    }

    /// Return the opaque transport identity stable across redelivery.
    #[must_use]
    pub fn message_key(&self) -> &str {
        &self.message_key
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

fn validate_delivery_text(value: &str, reason: &'static str) -> Result<(), ConsumeError> {
    if value.is_empty()
        || value.len() > MAX_DELIVERY_TEXT_BYTES
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(ConsumeError::protocol(reason))
    } else {
        Ok(())
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
    RetryAfter(RetryDelay),
    /// Stop redelivery only after durable quarantine evidence has committed.
    Quarantined,
}

/// Portable, prevalidated delay for requesting broker redelivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryDelay(Duration);

impl RetryDelay {
    /// Minimum supported delayed redelivery interval.
    pub const MIN: Duration = Duration::from_millis(1);
    /// Maximum supported delayed redelivery interval.
    pub const MAX: Duration = Duration::from_secs(24 * 60 * 60);

    /// Validate the delay before a delivery is consumed for settlement.
    ///
    /// # Errors
    ///
    /// Returns `InvalidDisposition` for a delay outside 1 millisecond to 24 hours.
    pub fn new(delay: Duration) -> Result<Self, ConsumeError> {
        if delay < Self::MIN || delay > Self::MAX {
            Err(ConsumeError::invalid_disposition(
                "retry delay must be between 1 millisecond and 24 hours",
            ))
        } else {
            Ok(Self(delay))
        }
    }

    /// Return the delay accepted by the portable settlement contract.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// Caller-actionable delivery and settlement failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsumeErrorKind {
    /// The consumer stream is unavailable; retry receive under service policy.
    Unavailable,
    /// Broker delivery metadata violates the portable contract.
    Protocol,
    /// A retry delay was rejected before constructing a settlement disposition.
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
    use super::{
        ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliveryMetadata, DeliverySettlement,
        MessageDelivery, RetryDelay, SettlementFuture,
    };
    use std::time::Duration;

    struct NoopSettlement;

    impl DeliverySettlement for NoopSettlement {
        fn settle(self: Box<Self>, _disposition: DeliveryDisposition) -> SettlementFuture {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn delivery_debug_omits_payload_bytes() -> Result<(), ConsumeError> {
        let payload = b"private-delivery-sentinel-7391";
        let metadata = DeliveryMetadata::new("orders:11", "events.subject", 1, 0, 11, 13)?;
        let delivery = MessageDelivery::new(payload.to_vec(), metadata, Box::new(NoopSettlement));

        let rendered = format!("{delivery:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-delivery-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(delivery.payload(), payload);
        Ok(())
    }

    #[test]
    fn retry_delay_validates_before_delivery_is_consumed() {
        for valid in [RetryDelay::MIN, RetryDelay::MAX] {
            assert!(matches!(RetryDelay::new(valid), Ok(delay) if delay.get() == valid));
        }
        for invalid in [
            Duration::ZERO,
            RetryDelay::MIN - Duration::from_nanos(1),
            RetryDelay::MAX + Duration::from_nanos(1),
        ] {
            assert_eq!(
                RetryDelay::new(invalid).err().map(|error| error.kind()),
                Some(ConsumeErrorKind::InvalidDisposition)
            );
        }
    }

    #[test]
    fn delivery_metadata_requires_a_subject_and_positive_attempt() {
        assert_eq!(
            DeliveryMetadata::new("stream:1", "", 1, 0, 1, 1)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            DeliveryMetadata::new(
                "stream:1",
                "edgeagent.command.execution.submit.v1",
                0,
                0,
                1,
                1,
            )
            .err()
            .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            DeliveryMetadata::new("", "edgeagent.command.execution.submit.v1", 1, 0, 1, 1,)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        let metadata = DeliveryMetadata::new(
            "orders:11",
            "edgeagent.command.execution.submit.v1",
            2,
            7,
            11,
            13,
        );
        assert_eq!(
            metadata
                .map(|metadata| {
                    (
                        metadata.message_key().to_owned(),
                        metadata.delivery_attempt(),
                        metadata.pending(),
                    )
                })
                .map_err(|error| error.kind()),
            Ok(("orders:11".to_owned(), 2, 7))
        );
    }
}
