//! Exact-byte poison-message retention and idempotent observation.

use crate::validation::{
    MAX_DELIVERY_KEY_BYTES, MAX_FAILURE_CODE_BYTES, MAX_TRANSPORT_SUBJECT_BYTES,
    validate_consumer_name, validate_token, validate_visible_ascii,
};
use crate::{InboxError, PostgresInbox};
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use sqlx::{Postgres, Transaction};

/// Idempotent result of durably retaining a terminal inbound delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineDisposition {
    /// This transport delivery was retained for the first time.
    Inserted,
    /// Identical evidence was already retained, typically after lost settlement confirmation.
    AlreadyPresent,
}

/// Validated poison-message evidence retained before terminal broker settlement.
/// `Debug` reports the payload size without exposing its bytes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct QuarantineEvidence<'delivery> {
    delivery_key: &'delivery str,
    transport_subject: &'delivery str,
    delivery_attempt: i64,
    payload: &'delivery [u8],
    failure_code: &'delivery str,
}

impl std::fmt::Debug for QuarantineEvidence<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QuarantineEvidence")
            .field("delivery_attempt", &self.delivery_attempt)
            .field("failure_code", &self.failure_code)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
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
        if payload.len() > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(InboxError::invalid_quarantine_evidence(
                "quarantine payload must not exceed 256 KiB",
            ));
        }
        if delivery_attempt == 0 {
            return Err(InboxError::invalid_quarantine_evidence(
                "delivery_attempt must be positive",
            ));
        }
        Ok(Self {
            delivery_key,
            transport_subject,
            delivery_attempt: i64::from(delivery_attempt),
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

impl PostgresInbox {
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
        transaction: &mut Transaction<'_, Postgres>,
        consumer_name: &str,
        evidence: QuarantineEvidence<'_>,
    ) -> Result<QuarantineDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        let inserted = sqlx::query!(
            r#"
            INSERT INTO edgeagent_message_quarantine (
                consumer_name, delivery_key, transport_subject, payload,
                failure_code, first_delivery_attempt, last_delivery_attempt
            )
            VALUES ($1, $2, $3, $4, $5, $6, $6)
            ON CONFLICT (consumer_name, delivery_key) DO NOTHING
            "#,
            consumer_name,
            evidence.delivery_key,
            evidence.transport_subject,
            evidence.payload,
            evidence.failure_code,
            evidence.delivery_attempt,
        )
        .execute(&mut **transaction)
        .await
        .map_err(InboxError::storage)?;
        if inserted.rows_affected() == 1 {
            return Ok(QuarantineDisposition::Inserted);
        }

        let existing = sqlx::query!(
            r#"
            SELECT (
                transport_subject = $3 AND payload = $4 AND failure_code = $5
            ) AS "matches!"
            FROM edgeagent_message_quarantine
            WHERE consumer_name = $1 AND delivery_key = $2
            FOR UPDATE
            "#,
            consumer_name,
            evidence.delivery_key,
            evidence.transport_subject,
            evidence.payload,
            evidence.failure_code,
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(InboxError::storage)?
        .ok_or_else(InboxError::storage_invariant)?;
        if !existing.matches {
            return Err(InboxError::quarantine_identity_conflict());
        }
        let updated = sqlx::query!(
            r#"
            UPDATE edgeagent_message_quarantine
            SET last_delivery_attempt = GREATEST(last_delivery_attempt, $3),
                last_observed_at = clock_timestamp()
            WHERE consumer_name = $1 AND delivery_key = $2
            "#,
            consumer_name,
            evidence.delivery_key,
            evidence.delivery_attempt,
        )
        .execute(&mut **transaction)
        .await
        .map_err(InboxError::storage)?;
        if updated.rows_affected() != 1 {
            return Err(InboxError::storage_invariant());
        }
        Ok(QuarantineDisposition::AlreadyPresent)
    }
}
