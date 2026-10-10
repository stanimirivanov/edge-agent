//! Application-owned ports and values for transactional inbound processing.

use edgeagent_contracts::{MAX_PORTABLE_MESSAGE_BYTES, MessageEnvelope, MessageRegistry};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use crate::{DeliveryMetadata, FailureCode};

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
/// The metadata has already been validated at delivery ingress. `Debug`
/// reports the payload size without exposing its bytes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct InboundQuarantine<'delivery> {
    metadata: &'delivery DeliveryMetadata,
    payload: &'delivery [u8],
    failure_code: FailureCode,
}

impl std::fmt::Debug for InboundQuarantine<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InboundQuarantine")
            .field("delivery_attempt", &self.metadata.delivery_attempt())
            .field("failure_code", &self.failure_code.as_str())
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl<'delivery> InboundQuarantine<'delivery> {
    /// Construct portable, bounded quarantine evidence.
    ///
    /// # Errors
    ///
    /// Returns `Contract` if payload bytes exceed the portable envelope limit.
    pub fn new(
        metadata: &'delivery DeliveryMetadata,
        payload: &'delivery [u8],
        failure_code: FailureCode,
    ) -> Result<Self, InboxStoreError> {
        if payload.len() > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(InboxStoreError::contract(
                "quarantine payload must not exceed 256 KiB",
            ));
        }
        Ok(Self {
            metadata,
            payload,
            failure_code,
        })
    }

    /// Return the opaque transport identity stable across redelivery.
    #[must_use]
    pub fn delivery_key(&self) -> &str {
        self.metadata.message_key()
    }

    /// Return the transport subject that selected the delivery.
    #[must_use]
    pub fn transport_subject(&self) -> &str {
        self.metadata.subject()
    }

    /// Return the one-based transport delivery attempt.
    #[must_use]
    pub const fn delivery_attempt(&self) -> u32 {
        self.metadata.delivery_attempt()
    }

    /// Return the exact untrusted delivery bytes.
    #[must_use]
    pub const fn payload(&self) -> &[u8] {
        self.payload
    }

    /// Return the bounded terminal reason code.
    #[must_use]
    pub const fn failure_code(&self) -> &str {
        self.failure_code.as_str()
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
pub struct HandlerFailure {
    kind: HandlerFailureKind,
    code: FailureCode,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl std::fmt::Debug for HandlerFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandlerFailure")
            .field("kind", &self.kind)
            .field("source_present", &self.source.is_some())
            .finish()
    }
}

impl HandlerFailure {
    /// Construct a transient failure without exposing internal details.
    #[must_use]
    pub const fn transient(code: FailureCode) -> Self {
        Self {
            kind: HandlerFailureKind::Transient,
            code,
            source: None,
        }
    }

    /// Construct a permanent failure without exposing internal details.
    #[must_use]
    pub const fn permanent(code: FailureCode) -> Self {
        Self {
            kind: HandlerFailureKind::Permanent,
            code,
            source: None,
        }
    }

    /// Construct a classified failure while preserving its internal cause.
    #[must_use]
    pub fn with_source<E>(kind: HandlerFailureKind, code: FailureCode, source: E) -> Self
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

    /// Return the validated quarantine reason code.
    #[must_use]
    pub const fn code(&self) -> FailureCode {
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
    /// The message or evidence failed contract validation.
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

/// Inbound storage failure with bounded text and an optional internal cause.
pub struct InboxStoreError {
    kind: InboxStoreErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl std::fmt::Debug for InboxStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InboxStoreError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .field("source_present", &self.source.is_some())
            .finish()
    }
}

impl InboxStoreError {
    /// Construct a classified failure without an underlying cause.
    ///
    /// Public formatting reports only the category and `source()` returns
    /// `None`. Use [`Self::with_source`] when a concrete cause exists rather
    /// than discarding it or inventing a placeholder error.
    ///
    /// ```
    /// use edgeagent_messaging::{InboxStoreError, InboxStoreErrorKind};
    /// use std::error::Error;
    ///
    /// let error = InboxStoreError::new(InboxStoreErrorKind::MessageIdentityConflict);
    /// assert_eq!(error.kind(), InboxStoreErrorKind::MessageIdentityConflict);
    /// assert!(error.source().is_none());
    /// ```
    ///
    /// A constructed failure must be used:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use edgeagent_messaging::{InboxStoreError, InboxStoreErrorKind};
    /// InboxStoreError::new(InboxStoreErrorKind::Invariant);
    /// ```
    #[must_use]
    pub const fn new(kind: InboxStoreErrorKind) -> Self {
        Self {
            kind,
            reason: None,
            source: None,
        }
    }

    /// Construct a classified adapter failure while preserving its cause.
    ///
    /// A wrapped failure must also be used:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use edgeagent_messaging::{InboxStoreError, InboxStoreErrorKind};
    /// InboxStoreError::with_source(
    ///     InboxStoreErrorKind::Unavailable, std::io::Error::other("example cause"),
    /// );
    /// ```
    #[must_use]
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

    const fn contract(reason: &'static str) -> Self {
        Self {
            kind: InboxStoreErrorKind::Contract,
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

#[cfg(test)]
mod tests {
    use super::{InboundQuarantine, InboxStoreErrorKind};
    use crate::{
        DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata, DeliverySubject, FailureCode,
    };
    use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;

    const POISON: FailureCode = FailureCode::from_static("poison");

    fn metadata() -> Result<DeliveryMetadata, Box<dyn std::error::Error>> {
        Ok(DeliveryMetadata::new(
            DeliveryMessageKey::new("delivery-01")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(1)?,
        ))
    }

    #[test]
    fn quarantine_evidence_is_bounded_before_adapter_work() -> Result<(), Box<dyn std::error::Error>>
    {
        let metadata = metadata()?;
        let accepted = vec![0; MAX_PORTABLE_MESSAGE_BYTES];
        let oversized = vec![0; MAX_PORTABLE_MESSAGE_BYTES + 1];
        let evidence = InboundQuarantine::new(&metadata, &accepted, POISON)?;
        assert_eq!(evidence.payload().len(), MAX_PORTABLE_MESSAGE_BYTES);
        assert_eq!(evidence.delivery_key(), "delivery-01");
        assert_eq!(evidence.transport_subject(), "events.subject");
        assert_eq!(evidence.delivery_attempt(), 1);
        assert_eq!(evidence.failure_code(), "poison");

        let result = InboundQuarantine::new(&metadata, &oversized, POISON);

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(InboxStoreErrorKind::Contract)
        );
        Ok(())
    }

    #[test]
    fn quarantine_debug_omits_payload_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let payload = b"private-payload-sentinel-7391";
        let metadata = metadata()?;
        let evidence = InboundQuarantine::new(&metadata, payload, POISON)?;

        let rendered = format!("{evidence:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-payload-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(evidence.payload(), payload);
        Ok(())
    }
}
