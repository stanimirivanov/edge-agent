//! Transactional PostgreSQL inbox deduplication for message consumers.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use tokio_postgres::Transaction;

const MAX_CONSUMER_NAME_BYTES: usize = 128;

/// Result of recording a delivery inside the handler's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// This consumer has not committed this message identity before.
    FirstDelivery,
    /// This consumer already committed the identical message.
    Duplicate,
}

/// PostgreSQL inbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresInbox;

impl PostgresInbox {
    /// SQL migration applied within each consumer-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_inbox.sql");

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

#[cfg(test)]
mod tests {
    use super::{InboxErrorKind, PostgresInbox, validate_consumer_name};

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
    }
}
