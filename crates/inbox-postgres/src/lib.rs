//! Transactional PostgreSQL inbox deduplication for message consumers.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use tokio_postgres::Transaction;

const MAX_CONSUMER_NAME_BYTES: usize = 128;
const MAX_DELIVERY_KEY_BYTES: usize = 512;
const MAX_TRANSPORT_SUBJECT_BYTES: usize = 512;
const MAX_FAILURE_CODE_BYTES: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

const INSERT_QUARANTINE_SQL: &str = r#"
INSERT INTO edgeagent_message_quarantine (
    consumer_name,
    delivery_key,
    transport_subject,
    payload,
    failure_code,
    first_delivery_attempt,
    last_delivery_attempt
)
VALUES ($1, $2, $3, $4, $5, $6, $6)
ON CONFLICT (consumer_name, delivery_key) DO NOTHING
"#;

const UPDATE_QUARANTINE_OBSERVATION_SQL: &str = r#"
UPDATE edgeagent_message_quarantine
SET last_delivery_attempt = GREATEST(last_delivery_attempt, $3),
    last_observed_at = clock_timestamp()
WHERE consumer_name = $1 AND delivery_key = $2
"#;

/// Result of recording a delivery inside the handler's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// This consumer has not committed this message identity before.
    FirstDelivery,
    /// This consumer already committed the identical message.
    Duplicate,
}

/// Idempotent result of durably retaining a terminal inbound delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineDisposition {
    /// This transport delivery was retained for the first time.
    Inserted,
    /// Identical evidence was already retained, typically after lost settlement confirmation.
    AlreadyPresent,
}

/// Validated poison-message evidence retained before terminal broker settlement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuarantineEvidence<'delivery> {
    delivery_key: &'delivery str,
    transport_subject: &'delivery str,
    delivery_attempt: i32,
    payload: &'delivery [u8],
    failure_code: &'delivery str,
}

impl<'delivery> QuarantineEvidence<'delivery> {
    /// Construct bounded quarantine evidence without interpreting untrusted payload bytes.
    ///
    /// # Errors
    ///
    /// Returns `InvalidQuarantineEvidence` when identity, subject, attempt,
    /// payload size, or reason-code bounds are violated.
    pub fn new(
        delivery_key: &'delivery str,
        transport_subject: &'delivery str,
        delivery_attempt: u32,
        payload: &'delivery [u8],
        failure_code: &'delivery str,
    ) -> Result<Self, InboxError> {
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
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(InboxError::invalid_quarantine_evidence(
                "quarantine payload must not exceed 256 KiB",
            ));
        }
        let delivery_attempt = i32::try_from(delivery_attempt).map_err(|_| {
            InboxError::invalid_quarantine_evidence(
                "delivery_attempt must be between 1 and 2147483647",
            )
        })?;
        if delivery_attempt == 0 {
            return Err(InboxError::invalid_quarantine_evidence(
                "delivery_attempt must be between 1 and 2147483647",
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
}

/// PostgreSQL inbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresInbox;

impl PostgresInbox {
    /// SQL migration applied within each consumer-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_inbox.sql");

    /// Migration that adds retained inbound poison-message evidence.
    pub const QUARANTINE_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0002_message_quarantine.sql");

    /// Ordered migrations required by this adapter.
    pub const MIGRATIONS: [&'static str; 2] = [Self::MIGRATION_SQL, Self::QUARANTINE_MIGRATION_SQL];

    /// Validate and record a delivery before applying its domain transition.
    ///
    /// The caller MUST perform its domain writes in the same transaction and
    /// commit only when this method returns [`DeliveryDisposition::FirstDelivery`].
    /// A duplicate requires no domain work but may commit the no-op transaction
    /// before acknowledging the transport delivery.
    ///
    /// # Errors
    ///
    /// Returns a contract error for an unsupported or invalid envelope, an
    /// identity conflict when immutable bytes differ, an invalid consumer name,
    /// or a PostgreSQL/storage-invariant failure.
    pub async fn record_delivery(
        &self,
        transaction: &Transaction<'_>,
        consumer_name: &str,
        registry: &MessageRegistry<'_>,
        envelope: &MessageEnvelope,
    ) -> Result<DeliveryDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        registry.validate(envelope)?;
        let bytes = envelope.to_json().map_err(MessageRoutingError::from)?;
        let inserted = transaction
            .execute(
                "INSERT INTO edgeagent_message_inbox (consumer_name, message_source, message_id, message_type, envelope) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (consumer_name, message_source, message_id) DO NOTHING",
                &[&consumer_name, &envelope.source(), &envelope.id(), &envelope.message_type(), &bytes],
            )
            .await
            .map_err(InboxError::storage)?;
        if inserted == 1 {
            return Ok(DeliveryDisposition::FirstDelivery);
        }

        let existing = transaction
            .query_opt(
                "SELECT message_type, envelope FROM edgeagent_message_inbox WHERE consumer_name = $1 AND message_source = $2 AND message_id = $3",
                &[&consumer_name, &envelope.source(), &envelope.id()],
            )
            .await
            .map_err(InboxError::storage)?
            .ok_or_else(InboxError::storage_invariant)?;
        let existing_type: String = existing.try_get(0).map_err(InboxError::storage)?;
        let existing_bytes: Vec<u8> = existing.try_get(1).map_err(InboxError::storage)?;
        if existing_type == envelope.message_type() && existing_bytes == bytes {
            Ok(DeliveryDisposition::Duplicate)
        } else {
            Err(InboxError::message_identity_conflict())
        }
    }

    /// Retain exact poison-message evidence before terminal broker settlement.
    ///
    /// The caller owns this transaction and MUST commit it before settling the
    /// transport delivery as quarantined. Repeated identical evidence updates
    /// only the last observed attempt. Reuse of one delivery key with different
    /// immutable content fails closed.
    ///
    /// # Errors
    ///
    /// Returns an invalid consumer, evidence, identity-conflict, storage, or
    /// storage-invariant error.
    pub async fn quarantine_delivery(
        &self,
        transaction: &Transaction<'_>,
        consumer_name: &str,
        evidence: QuarantineEvidence<'_>,
    ) -> Result<QuarantineDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        let inserted = transaction
            .execute(
                INSERT_QUARANTINE_SQL,
                &[
                    &consumer_name,
                    &evidence.delivery_key,
                    &evidence.transport_subject,
                    &evidence.payload,
                    &evidence.failure_code,
                    &evidence.delivery_attempt,
                ],
            )
            .await
            .map_err(InboxError::storage)?;
        if inserted == 1 {
            return Ok(QuarantineDisposition::Inserted);
        }

        let existing = transaction
            .query_opt(
                "SELECT transport_subject, payload, failure_code FROM edgeagent_message_quarantine WHERE consumer_name = $1 AND delivery_key = $2 FOR UPDATE",
                &[&consumer_name, &evidence.delivery_key],
            )
            .await
            .map_err(InboxError::storage)?
            .ok_or_else(InboxError::storage_invariant)?;
        let existing_subject: String = existing.try_get(0).map_err(InboxError::storage)?;
        let existing_payload: Vec<u8> = existing.try_get(1).map_err(InboxError::storage)?;
        let existing_failure_code: String = existing.try_get(2).map_err(InboxError::storage)?;
        if existing_subject != evidence.transport_subject
            || existing_payload != evidence.payload
            || existing_failure_code != evidence.failure_code
        {
            return Err(InboxError::quarantine_identity_conflict());
        }
        let updated = transaction
            .execute(
                UPDATE_QUARANTINE_OBSERVATION_SQL,
                &[
                    &consumer_name,
                    &evidence.delivery_key,
                    &evidence.delivery_attempt,
                ],
            )
            .await
            .map_err(InboxError::storage)?;
        if updated != 1 {
            return Err(InboxError::storage_invariant());
        }
        Ok(QuarantineDisposition::AlreadyPresent)
    }
}

/// Stable failure categories for inbox callers and acknowledgement policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxErrorKind {
    /// The message is invalid or unsupported by the consumer registry.
    Contract,
    /// One consumer saw different immutable content under the same identity.
    MessageIdentityConflict,
    /// The stable consumer name violates its bounded token contract.
    InvalidConsumerName,
    /// Quarantine identity, bounds, or reason code are invalid.
    InvalidQuarantineEvidence,
    /// One transport delivery key was reused with different immutable evidence.
    QuarantineIdentityConflict,
    /// PostgreSQL rejected or could not complete an operation.
    Storage,
    /// Stored columns violate invariants expected by this adapter.
    StorageInvariant,
}

impl Display for InboxErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("inbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("inbox message identity conflicts with stored content")
            }
            Self::InvalidConsumerName => formatter.write_str("inbox consumer name is invalid"),
            Self::InvalidQuarantineEvidence => {
                formatter.write_str("inbox quarantine evidence is invalid")
            }
            Self::QuarantineIdentityConflict => {
                formatter.write_str("inbox quarantine identity conflicts with stored evidence")
            }
            Self::Storage => formatter.write_str("inbox storage operation failed"),
            Self::StorageInvariant => formatter.write_str("inbox storage invariant failed"),
        }
    }
}

/// Inbox failure with bounded public text and an optional internal cause.
#[derive(Debug)]
pub struct InboxError {
    kind: InboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl InboxError {
    /// Return the stable category used by handler and acknowledgement policy.
    #[must_use]
    pub const fn kind(&self) -> InboxErrorKind {
        self.kind
    }

    const fn invalid_consumer_name(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidConsumerName,
            reason: Some(reason),
            source: None,
        }
    }

    const fn invalid_quarantine_evidence(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidQuarantineEvidence,
            reason: Some(reason),
            source: None,
        }
    }

    fn storage(error: tokio_postgres::Error) -> Self {
        Self {
            kind: InboxErrorKind::Storage,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    const fn storage_invariant() -> Self {
        Self {
            kind: InboxErrorKind::StorageInvariant,
            reason: None,
            source: None,
        }
    }

    const fn message_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::MessageIdentityConflict,
            reason: None,
            source: None,
        }
    }

    const fn quarantine_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::QuarantineIdentityConflict,
            reason: None,
            source: None,
        }
    }
}

impl Display for InboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for InboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<MessageRoutingError> for InboxError {
    fn from(error: MessageRoutingError) -> Self {
        Self {
            kind: InboxErrorKind::Contract,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

fn validate_consumer_name(value: &str) -> Result<(), InboxError> {
    if value.is_empty() || value.len() > MAX_CONSUMER_NAME_BYTES {
        return Err(InboxError::invalid_consumer_name(
            "consumer_name must contain 1 to 128 bytes",
        ));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(InboxError::invalid_consumer_name(
            "consumer_name must be a lowercase ASCII token",
        ));
    }
    Ok(())
}

fn validate_visible_ascii(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(InboxError::invalid_quarantine_evidence(reason))
    } else {
        Ok(())
    }
}

fn validate_token(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Err(InboxError::invalid_quarantine_evidence(reason))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InboxErrorKind, MAX_PAYLOAD_BYTES, PostgresInbox, QuarantineEvidence,
        validate_consumer_name,
    };

    #[test]
    fn consumer_names_are_bounded_stable_tokens() {
        assert!(validate_consumer_name("execution_simulator_v1").is_ok());
        assert_eq!(
            validate_consumer_name("Execution Simulator")
                .err()
                .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidConsumerName)
        );
        assert_eq!(
            validate_consumer_name(&"a".repeat(129))
                .err()
                .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidConsumerName)
        );
    }

    #[test]
    fn migration_scopes_identity_by_consumer_and_stores_exact_bytes() {
        assert!(PostgresInbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
        assert!(
            PostgresInbox::MIGRATION_SQL
                .contains("PRIMARY KEY (consumer_name, message_source, message_id)")
        );
        assert_eq!(PostgresInbox::MIGRATIONS.len(), 2);
        assert!(
            PostgresInbox::QUARANTINE_MIGRATION_SQL
                .contains("PRIMARY KEY (consumer_name, delivery_key)")
        );
        assert!(
            PostgresInbox::QUARANTINE_MIGRATION_SQL.contains("octet_length(payload) <= 262144")
        );
    }

    #[test]
    fn quarantine_evidence_is_bounded_before_database_work() {
        assert!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                b"not-json",
                "envelope_invalid",
            )
            .is_ok()
        );
        assert_eq!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                b"not-json",
                "Envelope Invalid",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidQuarantineEvidence)
        );
        let oversized = vec![0_u8; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                &oversized,
                "envelope_invalid",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidQuarantineEvidence)
        );
    }
}
