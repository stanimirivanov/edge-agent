//! Validated broker-delivery metadata shared by consumer adapters and handlers.

use std::num::{NonZeroU32, NonZeroU64};

use crate::ConsumeError;

const MAX_DELIVERY_TEXT_BYTES: usize = 512;

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

/// Opaque transport identity stable across redelivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryMessageKey(String);

impl DeliveryMessageKey {
    /// Parse a bounded visible-ASCII delivery identity.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for an empty, oversized, or non-graphic value.
    pub fn new(value: impl Into<String>) -> Result<Self, ConsumeError> {
        let value = value.into();
        validate_delivery_text(&value, "message key violates delivery text bounds")?;
        Ok(Self(value))
    }

    /// Return the validated opaque identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Subject that selected one broker delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliverySubject(String);

impl DeliverySubject {
    /// Parse a bounded visible-ASCII delivery subject.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for an empty, oversized, or non-graphic value.
    pub fn new(value: impl Into<String>) -> Result<Self, ConsumeError> {
        let value = value.into();
        validate_delivery_text(&value, "delivery subject violates delivery text bounds")?;
        Ok(Self(value))
    }

    /// Return the validated broker subject.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One-based count of attempts to deliver this message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryAttempt(NonZeroU32);

impl DeliveryAttempt {
    /// Parse a positive broker delivery attempt.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for zero.
    pub fn new(value: u32) -> Result<Self, ConsumeError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or_else(|| ConsumeError::protocol("delivery attempt must be greater than zero"))
    }

    /// Return the one-based attempt count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Positive stream sequence used for ordering diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSequence(NonZeroU64);

impl StreamSequence {
    /// Parse a positive stream sequence.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for zero.
    pub fn new(value: u64) -> Result<Self, ConsumeError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or_else(|| ConsumeError::protocol("stream sequence must be greater than zero"))
    }

    /// Return the stream sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Positive consumer sequence used for redelivery diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsumerSequence(NonZeroU64);

impl ConsumerSequence {
    /// Parse a positive consumer sequence.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for zero.
    pub fn new(value: u64) -> Result<Self, ConsumeError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or_else(|| ConsumeError::protocol("consumer sequence must be greater than zero"))
    }

    /// Return the consumer sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Portable metadata needed for idempotency, ordering diagnostics, and backpressure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryMetadata {
    message_key: DeliveryMessageKey,
    subject: DeliverySubject,
    delivery_attempt: DeliveryAttempt,
    pending: u64,
    stream_sequence: StreamSequence,
    consumer_sequence: ConsumerSequence,
}

impl DeliveryMetadata {
    /// Assemble metadata from values parsed at the broker adapter boundary.
    #[must_use]
    pub fn new(
        message_key: DeliveryMessageKey,
        subject: DeliverySubject,
        delivery_attempt: DeliveryAttempt,
        pending: u64,
        stream_sequence: StreamSequence,
        consumer_sequence: ConsumerSequence,
    ) -> Self {
        Self {
            message_key,
            subject,
            delivery_attempt,
            pending,
            stream_sequence,
            consumer_sequence,
        }
    }

    /// Return the opaque transport identity stable across redelivery.
    #[must_use]
    pub fn message_key(&self) -> &str {
        self.message_key.as_str()
    }

    /// Return the broker subject that selected this delivery.
    #[must_use]
    pub fn subject(&self) -> &str {
        self.subject.as_str()
    }

    /// Return the one-based broker delivery attempt.
    #[must_use]
    pub const fn delivery_attempt(&self) -> u32 {
        self.delivery_attempt.get()
    }

    /// Return messages known by the broker to be pending for this consumer.
    #[must_use]
    pub const fn pending(&self) -> u64 {
        self.pending
    }

    /// Return the broker stream sequence used for ordering diagnostics.
    #[must_use]
    pub const fn stream_sequence(&self) -> u64 {
        self.stream_sequence.get()
    }

    /// Return the consumer delivery sequence used for redelivery diagnostics.
    #[must_use]
    pub const fn consumer_sequence(&self) -> u64 {
        self.consumer_sequence.get()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConsumerSequence, DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata, DeliverySubject,
        StreamSequence,
    };
    use crate::ConsumeErrorKind;

    #[test]
    fn metadata_preserves_distinct_fields() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("orders:11")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(2)?,
            7,
            StreamSequence::new(11)?,
            ConsumerSequence::new(13)?,
        );
        assert_eq!(metadata.message_key(), "orders:11");
        assert_eq!(metadata.subject(), "events.subject");
        assert_eq!(metadata.delivery_attempt(), 2);
        assert_eq!(metadata.pending(), 7);
        assert_eq!(metadata.stream_sequence(), 11);
        assert_eq!(metadata.consumer_sequence(), 13);
        Ok(())
    }

    #[test]
    fn metadata_text_bounds_reject_untrusted_values() {
        for invalid in [
            String::new(),
            "a".repeat(513),
            "has space".to_owned(),
            "é".to_owned(),
            "line\nfeed".to_owned(),
        ] {
            assert_eq!(
                DeliveryMessageKey::new(invalid.clone())
                    .err()
                    .map(|error| error.kind()),
                Some(ConsumeErrorKind::Protocol)
            );
            assert_eq!(
                DeliverySubject::new(invalid)
                    .err()
                    .map(|error| error.kind()),
                Some(ConsumeErrorKind::Protocol)
            );
        }
        assert!(DeliveryMessageKey::new("a".repeat(512)).is_ok());
        assert!(DeliverySubject::new("a".repeat(512)).is_ok());
    }

    #[test]
    fn metadata_counters_reject_zero() {
        assert_eq!(
            DeliveryAttempt::new(0).err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            StreamSequence::new(0).err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            ConsumerSequence::new(0).err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
    }
}
