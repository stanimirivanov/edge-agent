//! Application-owned ports and values for transactional inbound processing.

use edgeagent_contracts::{MAX_PORTABLE_MESSAGE_BYTES, MessageEnvelope, MessageRegistry};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;

const MAX_DELIVERY_KEY_BYTES: usize = 512;
const MAX_TRANSPORT_SUBJECT_BYTES: usize = 512;
const MAX_FAILURE_CODE_BYTES: usize = 64;

/// Boxed future returned by an inbound processing port.
pub type InboxFuture<'operation, Output> =
    Pin<Box<dyn Future<Output = Output> + Send + 'operation>>;

/// Persistence-neutral capability required by one inbound coordinator.
///
/// Implementations own transaction boundaries. `process` must atomically
/// combine inbox deduplication with the first delivery's service-owned state
/// transition. `quarantine` must commit evidence before it returns success.
///
/// `Handler` means the inbox and service-owned transition were rolled back.
/// `Contract`, `MessageIdentityConflict`, and `Invariant` mean no service
/// transition committed.
/// `Unavailable` may represent an ambiguous commit, so callers retry the same
/// immutable message and rely on durable identity to resolve the outcome.
pub trait InboundMessageStore: Send {
    /// Atomically deduplicate and, for a first delivery, apply service-owned work.
    fn process<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        registry: &'operation MessageRegistry<'_>,
        envelope: &'operation MessageEnvelope,
    ) -> InboxFuture<'operation, Result<InboxDisposition, InboundProcessingError>>;

    /// Atomically retain terminal evidence before broker settlement.
    ///
    /// Success confirms commit. An `Unavailable` error may be ambiguous and
    /// MUST NOT be treated as permission for terminal broker settlement.
    fn quarantine<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        evidence: InboundQuarantine<'operation>,
    ) -> InboxFuture<'operation, Result<QuarantineDisposition, InboxStoreError>>;
}

/// Result of an atomic inbound delivery transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxDisposition {
    /// This consumer committed the message and its service-owned transition.
    Applied,
    /// This consumer had already committed the identical message.
    Duplicate,
}

/// Result of atomically retaining terminal inbound evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineDisposition {
    /// Evidence was retained for the first time.
    Inserted,
    /// Identical evidence already existed after a prior delivery.
    AlreadyPresent,
}

/// Bounded poison-message evidence supplied to an inbound storage adapter.
/// `Debug` reports the payload size without exposing its bytes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct InboundQuarantine<'delivery> {
    delivery_key: &'delivery str,
    transport_subject: &'delivery str,
    delivery_attempt: u32,
    payload: &'delivery [u8],
    failure_code: &'delivery str,
}

impl std::fmt::Debug for InboundQuarantine<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InboundQuarantine")
            .field("delivery_attempt", &self.delivery_attempt)
            .field("failure_code", &self.failure_code)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl<'delivery> InboundQuarantine<'delivery> {
    /// Construct portable, bounded quarantine evidence.
    ///
    /// # Errors
    ///
    /// Returns `Invariant` when evidence is outside the event-spine contract.
    pub fn new(
        delivery_key: &'delivery str,
        transport_subject: &'delivery str,
        delivery_attempt: u32,
        payload: &'delivery [u8],
        failure_code: &'delivery str,
    ) -> Result<Self, InboxStoreError> {
        validate_visible_ascii(
            delivery_key,
            MAX_DELIVERY_KEY_BYTES,
            "delivery_key must contain 1 to 512 visible ASCII bytes",
        )?;
        validate_visible_ascii(
            transport_subject,
            MAX_TRANSPORT_SUBJECT_BYTES,
            "transport_subject must contain 1 to 512 visible ASCII bytes",
        )?;
        validate_token(
            failure_code,
            MAX_FAILURE_CODE_BYTES,
            "failure_code must be a lowercase ASCII token of 1 to 64 bytes",
        )?;
        if delivery_attempt == 0 {
            return Err(InboxStoreError::invariant(
                "delivery_attempt must be greater than zero",
            ));
        }
        if payload.len() > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(InboxStoreError::invariant(
                "quarantine payload must not exceed 256 KiB",
            ));
        }
        Ok(Self {
            delivery_key,
            transport_subject,
            delivery_attempt,
            payload,
            failure_code,
        })
    }

    /// Return the opaque transport identity stable across redelivery.
    #[must_use]
    pub const fn delivery_key(&self) -> &str {
        self.delivery_key
    }

    /// Return the transport subject that selected the delivery.
    #[must_use]
    pub const fn transport_subject(&self) -> &str {
        self.transport_subject
    }

    /// Return the one-based transport delivery attempt.
    #[must_use]
    pub const fn delivery_attempt(&self) -> u32 {
        self.delivery_attempt
    }

    /// Return the exact untrusted delivery bytes.
    #[must_use]
    pub const fn payload(&self) -> &[u8] {
        self.payload
    }

    /// Return the bounded terminal reason code.
    #[must_use]
    pub const fn failure_code(&self) -> &str {
        self.failure_code
    }
}

/// Failure classification returned by service-owned transactional work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerFailureKind {
    /// The identical message may succeed on a later delivery.
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

/// Bounded service-owned failure returned from an adapter-managed transaction.
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

/// Stable categories exposed by an inbound storage adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxStoreErrorKind {
    /// The message failed the adapter's contract validation.
    Contract,
    /// One message identity was reused with different immutable content.
    MessageIdentityConflict,
    /// Storage was unavailable or the commit outcome is ambiguous.
    Unavailable,
    /// Configuration or retained state violated the port contract.
    Invariant,
}

impl Display for InboxStoreErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("inbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("inbox message identity conflicts with stored content")
            }
            Self::Unavailable => formatter.write_str("inbox storage is unavailable"),
            Self::Invariant => formatter.write_str("inbox storage invariant failed"),
        }
    }
}

/// Inbound storage failure with bounded text and a preserved internal cause.
#[derive(Debug)]
pub struct InboxStoreError {
    kind: InboxStoreErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl InboxStoreError {
    /// Construct a classified adapter failure while preserving its cause.
    pub fn with_source<E>(kind: InboxStoreErrorKind, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            reason: None,
            source: Some(Box::new(source)),
        }
    }

    /// Return the stable category used by inbound handling policy.
    #[must_use]
    pub const fn kind(&self) -> InboxStoreErrorKind {
        self.kind
    }

    const fn invariant(reason: &'static str) -> Self {
        Self {
            kind: InboxStoreErrorKind::Invariant,
            reason: Some(reason),
            source: None,
        }
    }
}

impl Display for InboxStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for InboxStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

/// Failure from an atomic inbound processing attempt.
#[derive(Debug)]
pub enum InboundProcessingError {
    /// Inbox persistence or transaction processing failed.
    Store(InboxStoreError),
    /// Service-owned work returned a classified failure and the transaction rolled back.
    Handler(HandlerFailure),
}

impl Display for InboundProcessingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::Handler(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for InboundProcessingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Handler(error) => Some(error),
        }
    }
}

fn validate_visible_ascii(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxStoreError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(InboxStoreError::invariant(reason))
    } else {
        Ok(())
    }
}

fn validate_token(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxStoreError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Err(InboxStoreError::invariant(reason))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InboundQuarantine, InboxStoreError, InboxStoreErrorKind};

    #[test]
    fn quarantine_evidence_is_bounded_before_adapter_work() {
        let result = InboundQuarantine::new("delivery-01", "events.subject", 0, b"{}", "poison");

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(InboxStoreErrorKind::Invariant)
        );
    }

    #[test]
    fn quarantine_debug_omits_payload_bytes() -> Result<(), InboxStoreError> {
        let payload = b"private-payload-sentinel-7391";
        let evidence =
            InboundQuarantine::new("delivery-01", "events.subject", 1, payload, "poison")?;

        let rendered = format!("{evidence:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-payload-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(evidence.payload(), payload);
        Ok(())
    }
}
