//! Delivery identity and deduplication inside the caller's transaction.

use crate::validation::validate_consumer_name;
use crate::{InboxError, PostgresInbox};
use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use sqlx::{Postgres, Transaction};

/// Result of recording a delivery inside the handler's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// This consumer has not committed this message identity before.
    FirstDelivery,
    /// This consumer already committed the identical message.
    Duplicate,
}

impl PostgresInbox {
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
        transaction: &mut Transaction<'_, Postgres>,
        consumer_name: &str,
        registry: &MessageRegistry<'_>,
        envelope: &MessageEnvelope,
    ) -> Result<DeliveryDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        registry.validate(envelope)?;
        let bytes = envelope.to_json().map_err(MessageRoutingError::from)?;
        let inserted = sqlx::query!(
            r#"
            INSERT INTO edgeagent_message_inbox
                (consumer_name, message_source, message_id, message_type, envelope)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (consumer_name, message_source, message_id) DO NOTHING
            "#,
            consumer_name,
            envelope.source(),
            envelope.id(),
            envelope.message_type(),
            &bytes,
        )
        .execute(&mut **transaction)
        .await
        .map_err(InboxError::storage)?;
        if inserted.rows_affected() == 1 {
            return Ok(DeliveryDisposition::FirstDelivery);
        }

        let existing = sqlx::query!(
            r#"
            SELECT (message_type = $4 AND envelope = $5) AS "matches!"
            FROM edgeagent_message_inbox
            WHERE consumer_name = $1 AND message_source = $2 AND message_id = $3
            "#,
            consumer_name,
            envelope.source(),
            envelope.id(),
            envelope.message_type(),
            &bytes,
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(InboxError::storage)?
        .ok_or_else(InboxError::storage_invariant)?;
        if existing.matches {
            Ok(DeliveryDisposition::Duplicate)
        } else {
            Err(InboxError::message_identity_conflict())
        }
    }
}
