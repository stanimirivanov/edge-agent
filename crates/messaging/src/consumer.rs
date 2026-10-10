//! Portable one-at-a-time delivery, metadata, and settlement contracts.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

mod delivery;

pub use delivery::MessageDelivery;

/// Future returned while waiting for one durable consumer delivery.
pub type ReceiveFuture<'consumer> =
    Pin<Box<dyn Future<Output = Result<MessageDelivery, ConsumeError>> + Send + 'consumer>>;

/// Future returned while confirming one delivery settlement with the broker.
///
/// Dropping an incomplete future does not undo a request or prove its outcome.
/// Await and inspect its result; if confirmation is unknown, permit redelivery
/// and rely on committed inbox identity rather than choosing another disposition.
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
    /// If broker metadata or payload size cannot be represented as a delivery, no
    /// `MessageDelivery` exists for handler-owned quarantine. A `Protocol`
    /// error is not a reason to acknowledge the raw message; callers must
    /// follow the adapter's documented recovery policy rather than blindly
    /// retrying intake.
    fn receive(&mut self) -> ReceiveFuture<'_>;
}

/// One-shot broker settlement owned by a received delivery.
pub trait DeliverySettlement: Send {
    /// Apply and confirm exactly one terminal disposition.
    ///
    /// Construction may perform synchronous adapter work. Dropping an unpolled
    /// or pending future cannot establish whether a broker action occurred and
    /// must not schedule an automatic replacement disposition. Return explicit
    /// failures through `ConsumeError`; callers retain idempotent recovery.
    #[must_use = "settlement must be awaited and its confirmation result inspected"]
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture;
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
    /// Broker delivery metadata or payload size violates the portable contract.
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
    use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    struct NoopSettlement;

    impl DeliverySettlement for NoopSettlement {
        fn settle(self: Box<Self>, _disposition: DeliveryDisposition) -> SettlementFuture {
            Box::pin(async { Ok(()) })
        }
    }

    fn complete_noop_settlement(delivery: MessageDelivery) {
        let mut future = delivery.settle(DeliveryDisposition::Acknowledge);
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            future.as_mut().poll(&mut context),
            Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn delivery_debug_omits_payload_bytes() -> Result<(), ConsumeError> {
        let payload = b"private-delivery-sentinel-7391";
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("orders:11")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(1)?,
        );
        let delivery = MessageDelivery::new(payload.to_vec(), metadata, Box::new(NoopSettlement))?;

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
        let delivery = MessageDelivery::new(payload.clone(), metadata, Box::new(NoopSettlement))?;

        assert_eq!(delivery.payload(), payload.as_ref());
        assert_eq!(delivery.payload().as_ptr(), payload.as_ptr());
        complete_noop_settlement(delivery);
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
    fn delivery_accepts_portable_limit_and_rejects_the_next_byte() -> Result<(), ConsumeError> {
        let metadata = || -> Result<DeliveryMetadata, ConsumeError> {
            Ok(DeliveryMetadata::new(
                DeliveryMessageKey::new("orders:13")?,
                DeliverySubject::new("events.subject")?,
                DeliveryAttempt::new(1)?,
            ))
        };
        let at_limit = Bytes::from(vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES]);
        let delivery =
            MessageDelivery::new(at_limit.clone(), metadata()?, Box::new(NoopSettlement))?;
        assert_eq!(delivery.payload().as_ptr(), at_limit.as_ptr());
        assert_eq!(delivery.payload().len(), MAX_PORTABLE_MESSAGE_BYTES);
        complete_noop_settlement(delivery);

        let over_limit = Bytes::from(vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES + 1]);
        assert_eq!(
            MessageDelivery::new(over_limit, metadata()?, Box::new(NoopSettlement))
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        Ok(())
    }
}
