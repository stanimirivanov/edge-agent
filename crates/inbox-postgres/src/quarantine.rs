//! Exact-byte poison-message retention and idempotent observation.

use crate::validation::{
    MAX_DELIVERY_KEY_BYTES, MAX_FAILURE_CODE_BYTES, MAX_TRANSPORT_SUBJECT_BYTES,
    validate_consumer_name, validate_token, validate_visible_ascii,
};
use crate::{InboxError, PostgresInbox};
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use tokio_postgres::Transaction;

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
    delivery_attempt: i32,
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
