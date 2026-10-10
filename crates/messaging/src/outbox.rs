//! Application-owned ports and values for durable outbound message relay.

use crate::FailureCode;
use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::pin::Pin;

mod timing;

pub use timing::{LeaseDuration, OutboxRetryDelay, OutboxTimingError};

/// Boxed future returned by an outbound relay storage port.
///
/// # Cancellation
///
/// Dropping an incomplete future does not confirm rollback or release a lease.
/// Recovery follows the operation's committed state and original claim fence;
/// see [`OutboxRelayStore`].
pub type OutboxStoreFuture<'operation, Output> =
    Pin<Box<dyn Future<Output = Result<Output, OutboxStoreError>> + Send + 'operation>>;

/// Persistence capabilities required by one outbound relay iteration.
///
/// Implementations own their transaction boundaries. A claim must be committed
/// before it is returned so publication never occurs inside a storage transaction.
/// Every outcome transition must compare the generation of that exact claim,
/// not only its worker identity; worker tokens can be reused after lease expiry.
/// Atomically compare message identity, owner, generation, unexpired lease, and
/// eligible record state. A rejected stale completion returns `StateTransition`
/// without changing a newer claim. Fencing cannot undo a broker publication.
pub trait OutboxRelayStore: Send {
    /// Claim at most one available message for this relay worker.
    /// The lifetime is validated before this operation is called.
    ///
    /// # Cancellation
    ///
    /// A lease may commit before the claim is returned. Cancellation must not
    /// start publication or release an unknown claim. A committed, abandoned
    /// lease remains unavailable until expiry; subsequent claim acquisition
    /// advances its generation, even when the worker token is reused. An
    /// unconfirmed claim is not evidence that the queue is empty.
    ///
    /// Raw durations cannot bypass the portable lifetime bound:
    ///
    /// ```compile_fail,E0308
    /// use edgeagent_messaging::OutboxRelayStore;
    /// use std::time::Duration;
    ///
    /// fn claim(store: &mut dyn OutboxRelayStore) {
    ///     let _ = store.claim_one("relay_01", Duration::from_secs(30));
    /// }
    /// ```
    fn claim_one<'operation>(
        &'operation mut self,
        lease_owner: &'operation str,
        lease_duration: LeaseDuration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>>;

    /// Record confirmed durable publication under the original lease.
    ///
    /// # Cancellation
    ///
    /// The published marker may already have committed. Do not infer either
    /// rollback or success, or automatically schedule retry/quarantine. Resolve
    /// through committed storage state or a later lease-controlled iteration.
    /// If the marker is absent, broker persistence may still have occurred;
    /// republish only the identical envelope after a valid claim is acquired.
    fn mark_published<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()>;

    /// Release a leased message for a bounded retry, including immediate eligibility.
    ///
    /// # Cancellation
    ///
    /// The release and retry schedule may already have committed. Cancellation
    /// does not justify a different outcome or bypass the persisted delay. A
    /// later iteration must acquire an eligible claim with a new generation;
    /// an old claim must never complete a subsequently reclaimed record.
    ///
    /// Raw durations cannot bypass the portable retry bound:
    ///
    /// ```compile_fail,E0308
    /// use edgeagent_messaging::{ClaimedMessage, FailureCode, OutboxRelayStore};
    /// use std::time::Duration;
    ///
    /// fn retry(store: &mut dyn OutboxRelayStore, claim: &ClaimedMessage) {
    ///     let _ = store.release_for_retry(
    ///         claim, "relay_01", Duration::ZERO,
    ///         FailureCode::from_static("transport_unavailable"),
    ///     );
    /// }
    /// ```
    fn release_for_retry<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()>;

    /// Move a leased message into terminal quarantine.
    ///
    /// # Cancellation
    ///
    /// Quarantine may already have committed. Do not automatically publish,
    /// retry, or release the record to compensate for an unknown result. Inspect
    /// committed state; a quarantined record requires separately authorized
    /// replay. If no outcome committed, normal lease expiry permits recovery.
    fn quarantine<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        reason: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()>;
}

/// Opaque, positive identity of one committed outbox claim.
///
/// Unlike the retry attempt count, this value must never reset on replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseGeneration(NonZeroU64);

impl LeaseGeneration {
    /// Construct a positive claim generation from a storage adapter.
    ///
    /// # Errors
    ///
    /// Returns `Invariant` when storage reports an unclaimed generation.
    pub fn new(value: u64) -> Result<Self, OutboxStoreError> {
        NonZeroU64::new(value).map(Self).ok_or_else(|| {
            OutboxStoreError::invariant("lease generation must be greater than zero")
        })
    }

    /// Return the numeric generation for adapter-side compare-and-set.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// One durable outbound message claimed for publication.
/// `Debug` reports the envelope size without exposing its bytes.
#[derive(Clone, Eq, PartialEq)]
pub struct ClaimedMessage {
    message_source: String,
    message_id: String,
    message_type: String,
    transport_subject: String,
    envelope: Vec<u8>,
    attempt: u32,
    lease_generation: LeaseGeneration,
}

impl std::fmt::Debug for ClaimedMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClaimedMessage")
            .field("message_type", &self.message_type)
            .field("attempt", &self.attempt)
            .field("lease_generation", &self.lease_generation)
            .field("envelope_bytes", &self.envelope.len())
            .finish_non_exhaustive()
    }
}

impl ClaimedMessage {
    /// Construct a claimed message from a persistence adapter.
    ///
    /// Stored contract fields remain untrusted and are validated before
    /// publication. The constructor requires one-based attempts and an already
    /// validated generation identifying this particular claim.
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
        lease_generation: LeaseGeneration,
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
            lease_generation,
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

    /// Return the exact stored envelope bytes without validating or modifying them.
    ///
    /// Persistence records remain untrusted; use [`Self::validated_envelope`]
    /// before publication.
    #[must_use]
    pub fn envelope_bytes(&self) -> &[u8] {
        &self.envelope
    }

    /// Return the one-based number of times this record has been claimed.
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Return the fence for this exact claim, including after owner-token reuse.
    #[must_use]
    pub const fn lease_generation(&self) -> LeaseGeneration {
        self.lease_generation
    }

    /// Decode and revalidate the stored bytes and denormalized routing metadata.
    ///
    /// Enforces the raw byte limit before decoding, then the registry's exact
    /// version, schema, partition, and event-producer policy. Finally compares
    /// stored `id`, `source`, `type`, and `transport_subject` with the validated
    /// envelope and derived route. This borrows the claim and leaves its exact
    /// bytes, attempt, and lease generation unchanged, including on rejection.
    ///
    /// # Errors
    ///
    /// Returns [`MessageRoutingError::Envelope`] for raw oversize, malformed,
    /// or invalid decoded input, [`MessageRoutingError::UnsupportedMessageType`]
    /// for an unregistered exact type, or a registry/column contract mismatch.
    /// Diagnostics preserve safe categories and field names, not compared
    /// values or payload bytes. The relay owns durable quarantine on rejection;
    /// this method performs no persistence or publication.
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

/// Outbound relay storage failure with bounded text and an optional internal cause.
pub struct OutboxStoreError {
    kind: OutboxStoreErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl std::fmt::Debug for OutboxStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboxStoreError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .field("source_present", &self.source.is_some())
            .finish()
    }
}

impl OutboxStoreError {
    /// Construct a classified failure without an underlying cause.
    ///
    /// Public formatting reports only the category and `source()` returns
    /// `None`. Use [`Self::with_source`] when a concrete cause exists rather
    /// than discarding it or inventing a placeholder error.
    ///
    /// ```
    /// use edgeagent_messaging::{OutboxStoreError, OutboxStoreErrorKind};
    /// use std::error::Error;
    ///
    /// let error = OutboxStoreError::new(OutboxStoreErrorKind::StateTransition);
    /// assert_eq!(error.kind(), OutboxStoreErrorKind::StateTransition);
    /// assert!(error.source().is_none());
    /// ```
    ///
    /// A constructed failure must be used:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use edgeagent_messaging::{OutboxStoreError, OutboxStoreErrorKind};
    /// OutboxStoreError::new(OutboxStoreErrorKind::Invariant);
    /// ```
    #[must_use]
    pub const fn new(kind: OutboxStoreErrorKind) -> Self {
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
    /// use edgeagent_messaging::{OutboxStoreError, OutboxStoreErrorKind};
    /// OutboxStoreError::with_source(
    ///     OutboxStoreErrorKind::Unavailable, std::io::Error::other("example cause"),
    /// );
    /// ```
    #[must_use]
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
        Err(MessageRoutingError::ContractMismatch { field })
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaimedMessage, LeaseGeneration, OutboxStoreError, OutboxStoreErrorKind};

    #[test]
    fn lease_generation_is_positive() -> Result<(), OutboxStoreError> {
        assert_eq!(
            LeaseGeneration::new(0).err().map(|error| error.kind()),
            Some(OutboxStoreErrorKind::Invariant)
        );
        assert_eq!(LeaseGeneration::new(1)?.get(), 1);
        Ok(())
    }

    #[test]
    fn claimed_message_attempt_is_one_based() -> Result<(), OutboxStoreError> {
        let result = ClaimedMessage::new(
            "urn:edgeagent:component:gateway".to_owned(),
            "message-01".to_owned(),
            "com.edgeagent.execution.submit-dry-run-order.v1".to_owned(),
            "edgeagent.command.execution.submit-dry-run-order.v1".to_owned(),
            b"{}".to_vec(),
            0,
            LeaseGeneration::new(1)?,
        );

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(OutboxStoreErrorKind::Invariant)
        );
        Ok(())
    }

    #[test]
    fn claimed_message_debug_omits_envelope_bytes() -> Result<(), OutboxStoreError> {
        let payload = b"private-envelope-sentinel-7391";
        let claim = ClaimedMessage::new(
            "urn:edgeagent:component:gateway".to_owned(),
            "message-01".to_owned(),
            "com.edgeagent.execution.submit-dry-run-order.v1".to_owned(),
            "edgeagent.command.execution.submit-dry-run-order.v1".to_owned(),
            payload.to_vec(),
            1,
            LeaseGeneration::new(1)?,
        )?;

        let rendered = format!("{claim:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-envelope-sentinel-7391"));
        assert!(rendered.contains("envelope_bytes"));
        assert_eq!(claim.envelope_bytes(), payload);
        Ok(())
    }
}
