//! Exact-byte transactional enqueue and immutable identity checks.

use crate::{OutboxError, PostgresOutbox};
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use tokio_postgres::Transaction;

/// Idempotent enqueue result inside the caller's database transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueDisposition {
    /// A new outbox record was inserted.
    Inserted,
    /// The same source, ID, routing metadata, and envelope bytes already exist.
    AlreadyPresent,
}

impl PostgresOutbox {
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
}
