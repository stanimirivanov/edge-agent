//! Transactional PostgreSQL outbox storage and relay leasing.

#![forbid(unsafe_code)]

use edgeagent_contracts::{
    MessageDefinition, MessageEnvelope, MessageRegistry, MessageRoutingError,
};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;
use tokio_postgres::{Row, Transaction};

const MAX_BATCH_SIZE: u16 = 1_000;
const MAX_LEASE_DURATION: Duration = Duration::from_secs(15 * 60);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_LEASE_OWNER_BYTES: usize = 128;
const MAX_FAILURE_CODE_BYTES: usize = 64;

/// Idempotent enqueue result inside the caller's database transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueDisposition {
    /// A new outbox record was inserted.
    Inserted,
    /// The same source, ID, routing metadata, and envelope bytes already exist.
    AlreadyPresent,
}

/// One unpublished outbox message leased to a relay instance.
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
    /// Return the CloudEvents producer source.
    #[must_use]
    pub fn message_source(&self) -> &str {
        &self.message_source
    }

    /// Return the CloudEvents message ID, unique within its source.
    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// Return the versioned CloudEvents type.
    #[must_use]
    pub fn message_type(&self) -> &str {
        &self.message_type
    }

    /// Return the definition-derived transport subject.
    #[must_use]
    pub fn transport_subject(&self) -> &str {
        &self.transport_subject
    }

    /// Return the exact structured CloudEvents bytes stored by the producer.
    #[must_use]
    pub fn envelope_bytes(&self) -> &[u8] {
        &self.envelope
    }

    /// Return the one-based number of times this record has been leased.
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

/// PostgreSQL outbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresOutbox;

impl PostgresOutbox {
    /// SQL migration applied within each service-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_outbox.sql");

    /// Insert an exact validated envelope in the caller's transaction.
    ///
    /// Reusing a `(source, id)` pair with identical routing metadata and bytes
    /// is idempotent. Reusing it with different content is a conflict.
    ///
    /// # Errors
    ///
    /// Returns a contract, identity-conflict, or PostgreSQL error.
    pub async fn enqueue(
        &self,
        transaction: &Transaction<'_>,
        definition: MessageDefinition,
        envelope: &MessageEnvelope,
    ) -> Result<EnqueueDisposition, OutboxError> {
        definition.validate_envelope(envelope)?;
        let transport_subject = definition.subject()?;
        let bytes = envelope.to_json().map_err(MessageRoutingError::from)?;
        let inserted = transaction
            .execute(
                "INSERT INTO edgeagent_message_outbox (message_source, message_id, message_type, transport_subject, envelope) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (message_source, message_id) DO NOTHING",
                &[&envelope.source(), &envelope.id(), &envelope.message_type(), &transport_subject, &bytes],
            )
            .await
            .map_err(OutboxError::storage)?;
        if inserted == 1 {
            return Ok(EnqueueDisposition::Inserted);
        }

        let existing = transaction
            .query_opt(
                "SELECT message_type, transport_subject, envelope FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
                &[&envelope.source(), &envelope.id()],
            )
            .await
            .map_err(OutboxError::storage)?
            .ok_or_else(OutboxError::storage_invariant)?;
        let existing_type: String = existing.try_get(0).map_err(OutboxError::storage)?;
        let existing_subject: String = existing.try_get(1).map_err(OutboxError::storage)?;
        let existing_bytes: Vec<u8> = existing.try_get(2).map_err(OutboxError::storage)?;
        if existing_type == envelope.message_type()
            && existing_subject == transport_subject
            && existing_bytes == bytes
        {
            Ok(EnqueueDisposition::AlreadyPresent)
        } else {
            Err(OutboxError::message_identity_conflict())
        }
    }

    /// Lease the next available records using PostgreSQL `SKIP LOCKED`.
    ///
    /// Concurrent relays receive disjoint records. Expired leases become
    /// eligible again, incrementing the attempt count on the next claim.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument, storage, or storage-invariant error.
    pub async fn claim_batch(
        &self,
        transaction: &Transaction<'_>,
        lease_owner: &str,
        batch_size: u16,
        lease_duration: Duration,
    ) -> Result<Vec<ClaimedMessage>, OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        if batch_size == 0 || batch_size > MAX_BATCH_SIZE {
            return Err(OutboxError::invalid_argument(
                "batch_size must be between 1 and 1000",
            ));
        }
        let lease_milliseconds = duration_milliseconds(
            "lease_duration",
            lease_duration,
            Duration::from_millis(1),
            MAX_LEASE_DURATION,
        )?;
        let rows = transaction
            .query(
                "WITH candidates AS (SELECT message_source, message_id FROM edgeagent_message_outbox WHERE published_at IS NULL AND available_at <= clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()) ORDER BY available_at, created_at, message_source, message_id FOR UPDATE SKIP LOCKED LIMIT $1) UPDATE edgeagent_message_outbox AS outbox SET lease_owner = $2, lease_expires_at = clock_timestamp() + ($3 * INTERVAL '1 millisecond'), attempt_count = outbox.attempt_count + 1 FROM candidates WHERE outbox.message_source = candidates.message_source AND outbox.message_id = candidates.message_id RETURNING outbox.message_source, outbox.message_id, outbox.message_type, outbox.transport_subject, outbox.envelope, outbox.attempt_count",
                &[&i64::from(batch_size), &lease_owner, &lease_milliseconds],
            )
            .await
            .map_err(OutboxError::storage)?;
        rows.into_iter().map(claimed_message).collect()
    }

    /// Mark a leased message as durably published and retain its audit record.
    ///
    /// # Errors
    ///
    /// Returns `LeaseLost` if the lease is absent, expired, or owned by another relay.
    pub async fn mark_published(
        &self,
        transaction: &Transaction<'_>,
        message_source: &str,
        message_id: &str,
        lease_owner: &str,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        let updated = transaction
            .execute(
                "UPDATE edgeagent_message_outbox SET published_at = clock_timestamp(), lease_owner = NULL, lease_expires_at = NULL, last_failure_code = NULL WHERE message_source = $1 AND message_id = $2 AND published_at IS NULL AND lease_owner = $3 AND lease_expires_at > clock_timestamp()",
                &[&message_source, &message_id, &lease_owner],
            )
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated)
    }

    /// Release a leased message after a classified failure and schedule retry.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument, storage, or lost-lease error.
    pub async fn release_for_retry(
        &self,
        transaction: &Transaction<'_>,
        message_source: &str,
        message_id: &str,
        lease_owner: &str,
        retry_after: Duration,
        failure_code: &str,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        validate_token("failure_code", failure_code, MAX_FAILURE_CODE_BYTES)?;
        let retry_milliseconds =
            duration_milliseconds("retry_after", retry_after, Duration::ZERO, MAX_RETRY_DELAY)?;
        let updated = transaction
            .execute(
                "UPDATE edgeagent_message_outbox SET available_at = clock_timestamp() + ($4 * INTERVAL '1 millisecond'), lease_owner = NULL, lease_expires_at = NULL, last_failure_code = $5 WHERE message_source = $1 AND message_id = $2 AND published_at IS NULL AND lease_owner = $3 AND lease_expires_at > clock_timestamp()",
                &[&message_source, &message_id, &lease_owner, &retry_milliseconds, &failure_code],
            )
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated)
    }
}

/// Stable failure categories for outbox callers and relay policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxErrorKind {
    /// The message violates its registered contract.
    Contract,
    /// One message identity was reused with different immutable content.
    MessageIdentityConflict,
    /// A bounded lease or retry argument is invalid.
    InvalidArgument,
    /// PostgreSQL rejected or could not complete an operation.
    Storage,
    /// Stored columns violate invariants expected by this adapter.
    StorageInvariant,
    /// A completion attempted to use an absent, expired, or foreign lease.
    LeaseLost,
}

impl Display for OutboxErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("outbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("outbox message identity conflicts with stored content")
            }
            Self::InvalidArgument => formatter.write_str("outbox argument is invalid"),
            Self::Storage => formatter.write_str("outbox storage operation failed"),
            Self::StorageInvariant => formatter.write_str("outbox storage invariant failed"),
            Self::LeaseLost => formatter.write_str("outbox relay lease was lost"),
        }
    }
}

/// Outbox failure with bounded public text and an optional internal cause.
#[derive(Debug)]
pub struct OutboxError {
    kind: OutboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl OutboxError {
    /// Return the stable category used by application and relay policy.
    #[must_use]
    pub const fn kind(&self) -> OutboxErrorKind {
        self.kind
    }

    const fn invalid_argument(reason: &'static str) -> Self {
        Self {
            kind: OutboxErrorKind::InvalidArgument,
            reason: Some(reason),
            source: None,
        }
    }

    fn storage(error: tokio_postgres::Error) -> Self {
        Self {
            kind: OutboxErrorKind::Storage,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    const fn storage_invariant() -> Self {
        Self {
            kind: OutboxErrorKind::StorageInvariant,
            reason: None,
            source: None,
        }
    }

    const fn message_identity_conflict() -> Self {
        Self {
            kind: OutboxErrorKind::MessageIdentityConflict,
            reason: None,
            source: None,
        }
    }

    const fn lease_lost() -> Self {
        Self {
            kind: OutboxErrorKind::LeaseLost,
            reason: None,
            source: None,
        }
    }
}

impl Display for OutboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for OutboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<MessageRoutingError> for OutboxError {
    fn from(error: MessageRoutingError) -> Self {
        Self {
            kind: OutboxErrorKind::Contract,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

fn claimed_message(row: Row) -> Result<ClaimedMessage, OutboxError> {
    let attempt: i32 = row.try_get(5).map_err(OutboxError::storage)?;
    Ok(ClaimedMessage {
        message_source: row.try_get(0).map_err(OutboxError::storage)?,
        message_id: row.try_get(1).map_err(OutboxError::storage)?,
        message_type: row.try_get(2).map_err(OutboxError::storage)?,
        transport_subject: row.try_get(3).map_err(OutboxError::storage)?,
        envelope: row.try_get(4).map_err(OutboxError::storage)?,
        attempt: u32::try_from(attempt).map_err(|_| OutboxError::storage_invariant())?,
    })
}

fn validate_token(
    field: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), OutboxError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(OutboxError::invalid_argument(match field {
            "lease_owner" => "lease_owner must contain 1 to 128 bytes",
            _ => "failure_code must contain 1 to 64 bytes",
        }));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(OutboxError::invalid_argument(match field {
            "lease_owner" => "lease_owner must be a lowercase ASCII token",
            _ => "failure_code must be a lowercase ASCII token",
        }));
    }
    Ok(())
}

fn duration_milliseconds(
    field: &'static str,
    value: Duration,
    minimum: Duration,
    maximum: Duration,
) -> Result<i64, OutboxError> {
    if value < minimum || value > maximum {
        return Err(OutboxError::invalid_argument(match field {
            "lease_duration" => "lease_duration must be between 1 millisecond and 15 minutes",
            _ => "retry_after must not exceed 24 hours",
        }));
    }
    i64::try_from(value.as_millis())
        .map_err(|_| OutboxError::invalid_argument("duration exceeds PostgreSQL range"))
}

fn require_updated(updated: u64) -> Result<(), OutboxError> {
    if updated == 1 {
        Ok(())
    } else {
        Err(OutboxError::lease_lost())
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
    use super::{
        MAX_BATCH_SIZE, MAX_LEASE_DURATION, OutboxErrorKind, PostgresOutbox, duration_milliseconds,
        validate_token,
    };
    use std::time::Duration;

    #[test]
    fn lease_arguments_are_bounded_before_database_work() {
        assert!(validate_token("lease_owner", "relay_01", 128).is_ok());
        assert_eq!(
            validate_token("lease_owner", "Relay 01", 128)
                .err()
                .map(|error| error.kind()),
            Some(OutboxErrorKind::InvalidArgument)
        );
        assert_eq!(MAX_BATCH_SIZE, 1_000);
        assert_eq!(
            duration_milliseconds(
                "lease_duration",
                MAX_LEASE_DURATION + Duration::from_millis(1),
                Duration::from_millis(1),
                MAX_LEASE_DURATION,
            )
            .err()
            .map(|error| error.kind()),
            Some(OutboxErrorKind::InvalidArgument)
        );
    }

    #[test]
    fn migration_uses_exact_bytes_and_source_scoped_identity() {
        assert!(PostgresOutbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
        assert!(PostgresOutbox::MIGRATION_SQL.contains("PRIMARY KEY (message_source, message_id)"));
    }
}
