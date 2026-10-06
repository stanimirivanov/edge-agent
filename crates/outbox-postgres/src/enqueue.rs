//! Exact-byte transactional enqueue and immutable identity checks.

use crate::{OutboxError, PostgresOutbox};
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use sqlx::{Postgres, Transaction};

const INSERT_OUTBOX_SQL: &str = r"
    INSERT INTO edgeagent_message_outbox (
        message_source, message_id, message_type, transport_subject, envelope
    ) VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (message_source, message_id) DO NOTHING
";

// A separate statement sees a concurrently committed conflicting insert under
// READ COMMITTED after ON CONFLICT has waited for that transaction to finish.
// Return only the comparison result; never transfer the stored envelope back.
const MATCH_OUTBOX_SQL: &str = r"
    SELECT (
        message_type = $3
        AND transport_subject = $4
        AND envelope = $5
    ) AS identical
    FROM edgeagent_message_outbox
    WHERE message_source = $1 AND message_id = $2
";

struct PreparedEnqueue<'envelope> {
    source: &'envelope str,
    id: &'envelope str,
    message_type: &'envelope str,
    transport_subject: String,
    bytes: Vec<u8>,
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

fn existing_disposition(identical: bool) -> Result<EnqueueDisposition, OutboxError> {
    if identical {
        Ok(EnqueueDisposition::AlreadyPresent)
    } else {
        Err(OutboxError::message_identity_conflict())
    }
}

impl PostgresOutbox {
    /// Insert an exact validated envelope in the caller's SQLx transaction.
    ///
    /// Reusing a `(source, id)` pair with identical routing metadata and bytes
    /// is idempotent. Reusing it with different content is a conflict. An
    /// inbound callback passes its existing transaction so its state and the
    /// outbound intent commit or roll back together.
    ///
    /// # Errors
    ///
    /// Returns a contract, identity-conflict, or PostgreSQL error.
    pub async fn enqueue(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
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

        let identical = sqlx::query_scalar::<_, bool>(MATCH_OUTBOX_SQL)
            .bind(prepared.source)
            .bind(prepared.id)
            .bind(prepared.message_type)
            .bind(&prepared.transport_subject)
            .bind(&prepared.bytes)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?
            .ok_or_else(OutboxError::storage_invariant)?;
        existing_disposition(identical)
    }
}
