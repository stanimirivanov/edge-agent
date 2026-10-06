//! Exact-byte transactional enqueue and immutable identity checks.

use crate::{OutboxError, PostgresOutbox};
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use sqlx::{Postgres, Transaction as SqlxTransaction};
use tokio_postgres::Transaction;

const INSERT_OUTBOX_SQL: &str = "INSERT INTO edgeagent_message_outbox (message_source, message_id, message_type, transport_subject, envelope) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (message_source, message_id) DO NOTHING";
const SELECT_OUTBOX_SQL: &str = "SELECT message_type, transport_subject, envelope FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2";

#[derive(sqlx::FromRow)]
struct StoredOutbox {
    message_type: String,
    transport_subject: String,
    envelope: Vec<u8>,
}

struct PreparedEnqueue<'envelope> {
    source: &'envelope str,
    id: &'envelope str,
    message_type: &'envelope str,
    transport_subject: String,
    bytes: Vec<u8>,
}

impl PreparedEnqueue<'_> {
    fn matches(&self, message_type: &str, transport_subject: &str, bytes: &[u8]) -> bool {
        self.message_type == message_type
            && self.transport_subject == transport_subject
            && self.bytes == bytes
    }
}

fn prepare<'envelope>(
    definition: MessageDefinition,
    envelope: &'envelope MessageEnvelope,
) -> Result<PreparedEnqueue<'envelope>, OutboxError> {
    definition.validate_envelope(envelope)?;
    Ok(PreparedEnqueue {
        source: envelope.source(),
        id: envelope.id(),
        message_type: envelope.message_type(),
        transport_subject: definition.subject()?,
        bytes: envelope.to_json().map_err(MessageRoutingError::from)?,
    })
}

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
        let prepared = prepare(definition, envelope)?;
        let inserted = transaction
            .execute(
                INSERT_OUTBOX_SQL,
                &[
                    &prepared.source,
                    &prepared.id,
                    &prepared.message_type,
                    &prepared.transport_subject,
                    &prepared.bytes,
                ],
            )
            .await
            .map_err(OutboxError::storage)?;
        if inserted == 1 {
            return Ok(EnqueueDisposition::Inserted);
        }

        let existing = transaction
            .query_opt(SELECT_OUTBOX_SQL, &[&prepared.source, &prepared.id])
            .await
            .map_err(OutboxError::storage)?
            .ok_or_else(OutboxError::storage_invariant)?;
        let existing_type: String = existing.try_get(0).map_err(OutboxError::storage)?;
        let existing_subject: String = existing.try_get(1).map_err(OutboxError::storage)?;
        let existing_bytes: Vec<u8> = existing.try_get(2).map_err(OutboxError::storage)?;
        if prepared.matches(&existing_type, &existing_subject, &existing_bytes) {
            Ok(EnqueueDisposition::AlreadyPresent)
        } else {
            Err(OutboxError::message_identity_conflict())
        }
    }

    /// Enqueue through the same SQLx transaction used by the PostgreSQL inbox.
    ///
    /// A service callback can write its own tables and call this method before
    /// the inbound adapter commits. Both records then commit or roll back as
    /// one unit. The relay's existing driver and portable policy are unchanged.
    ///
    /// # Errors
    ///
    /// Returns a contract, identity-conflict, or PostgreSQL error.
    pub async fn enqueue_sqlx(
        &self,
        transaction: &mut SqlxTransaction<'_, Postgres>,
        definition: MessageDefinition,
        envelope: &MessageEnvelope,
    ) -> Result<EnqueueDisposition, OutboxError> {
        let prepared = prepare(definition, envelope)?;
        let inserted = sqlx::query(INSERT_OUTBOX_SQL)
            .bind(prepared.source)
            .bind(prepared.id)
            .bind(prepared.message_type)
            .bind(&prepared.transport_subject)
            .bind(&prepared.bytes)
            .execute(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?;
        if inserted.rows_affected() == 1 {
            return Ok(EnqueueDisposition::Inserted);
        }

        let existing = sqlx::query_as::<_, StoredOutbox>(SELECT_OUTBOX_SQL)
            .bind(prepared.source)
            .bind(prepared.id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?
            .ok_or_else(OutboxError::storage_invariant)?;
        if prepared.matches(
            &existing.message_type,
            &existing.transport_subject,
            &existing.envelope,
        ) {
            Ok(EnqueueDisposition::AlreadyPresent)
        } else {
            Err(OutboxError::message_identity_conflict())
        }
    }
}
