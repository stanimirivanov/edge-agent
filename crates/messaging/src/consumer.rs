//! Portable one-at-a-time delivery, metadata, and settlement contracts.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;

use crate::metadata::DeliveryMetadata;

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
    /// If the broker already delivered it, redelivery after ack-wait can
    /// increment `delivery_attempt`; shutdown cancellation must not be treated
    /// as a handler failure when applying attempt-based policy.
    /// The returned delivery remains unsettled until the caller explicitly
    /// chooses one terminal disposition.
    /// If broker metadata cannot be represented as a delivery, no
    /// `MessageDelivery` exists for handler-owned quarantine. A `Protocol`
    /// error is not a reason to acknowledge the raw message; callers must
    /// follow the adapter's documented recovery policy rather than blindly
    /// retrying intake.
    fn receive(&mut self) -> ReceiveFuture<'_>;
}

/// One-shot broker settlement owned by a received delivery.
pub trait DeliverySettlement: Send {
    /// Apply and confirm exactly one terminal disposition.
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture;
}

/// An untrusted delivery whose settlement is consumed exactly once.
/// Dropping it before settlement emits a payload-safe warning and leaves broker
/// redelivery to the adapter; the warning is not a durable disposition.
pub struct MessageDelivery {
    payload: Bytes,
    metadata: DeliveryMetadata,
    settlement: Option<Box<dyn DeliverySettlement>>,
}

impl MessageDelivery {
    /// Construct a delivery from a transport adapter without copying a `Bytes`
    /// payload. Owned byte vectors remain accepted by existing callers.
    #[must_use]
    pub fn new(
        payload: impl Into<Bytes>,
        metadata: DeliveryMetadata,
        settlement: Box<dyn DeliverySettlement>,
    ) -> Self {
        Self {
            payload: payload.into(),
            metadata,
            settlement: Some(settlement),
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
    /// A lost confirmation permits redelivery; handlers must deduplicate by
    /// the stable message identity even after committing their local work.
    pub fn settle(mut self, disposition: DeliveryDisposition) -> SettlementFuture {
        match self.settlement.take() {
            Some(settlement) => settlement.settle(disposition),
            None => Box::pin(async {
                Err(ConsumeError::protocol(
                    "delivery settlement is missing after ownership transfer",
                ))
            }),
        }
    }
}

impl Drop for MessageDelivery {
    fn drop(&mut self) {
        if self.settlement.is_some() {
            tracing::warn!(
                delivery_attempt = self.metadata.delivery_attempt(),
                payload_bytes = self.payload.len(),
                "message delivery dropped without settlement"
            );
        }
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
pub struct ConsumeError {
    kind: ConsumeErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl std::fmt::Debug for ConsumeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConsumeError")
            .field("kind", &self.kind)
            .field("source_present", &self.source.is_some())
            .finish()
    }
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
        ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliverySettlement, MessageDelivery,
        RetryDelay, SettlementFuture,
    };
    use crate::metadata::{DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata, DeliverySubject};
    use bytes::Bytes;
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
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("orders:11")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(1)?,
        );
        let delivery = MessageDelivery::new(payload.to_vec(), metadata, Box::new(NoopSettlement));

        let rendered = format!("{delivery:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-delivery-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(delivery.payload(), payload);
        Ok(())
    }

    #[test]
    fn delivery_shares_bytes_payload_without_copying() -> Result<(), ConsumeError> {
        let payload = Bytes::from_static(b"private-shared-delivery-sentinel-7391");
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("orders:12")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(1)?,
        );
        let delivery = MessageDelivery::new(payload.clone(), metadata, Box::new(NoopSettlement));

        assert_eq!(delivery.payload(), payload.as_ref());
        assert_eq!(delivery.payload().as_ptr(), payload.as_ptr());
        drop(delivery.settle(DeliveryDisposition::Acknowledge));
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
}
