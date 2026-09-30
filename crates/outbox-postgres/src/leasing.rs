//! Claim, publish, retry, and quarantine transitions under expiring leases.

use crate::validation::{
    MAX_BATCH_SIZE, MAX_FAILURE_CODE_BYTES, MAX_LEASE_DURATION, MAX_LEASE_OWNER_BYTES,
    MAX_RETRY_DELAY, duration_milliseconds, validate_token,
};
use crate::{ClaimedMessage, OutboxError, PostgresOutbox};
use std::time::Duration;
use tokio_postgres::{Row, Transaction};

pub(super) const CLAIM_BATCH_SQL: &str = r#"
WITH candidates AS (
    SELECT message_source, message_id
    FROM edgeagent_message_outbox
    WHERE published_at IS NULL
      AND quarantined_at IS NULL
      AND available_at <= clock_timestamp()
      AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp())
    ORDER BY available_at, created_at, message_source, message_id
    FOR UPDATE SKIP LOCKED
    LIMIT $1
)
UPDATE edgeagent_message_outbox AS outbox
SET lease_owner = $2,
    lease_expires_at = clock_timestamp() + ($3::BIGINT * INTERVAL '1 millisecond'),
    attempt_count = outbox.attempt_count + 1
FROM candidates
WHERE outbox.message_source = candidates.message_source
  AND outbox.message_id = candidates.message_id
RETURNING outbox.message_source,
          outbox.message_id,
          outbox.message_type,
          outbox.transport_subject,
          outbox.envelope,
          outbox.attempt_count
"#;
pub(super) const RELEASE_FOR_RETRY_SQL: &str = r#"
UPDATE edgeagent_message_outbox
SET available_at = clock_timestamp() + ($4::BIGINT * INTERVAL '1 millisecond'),
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = $5
WHERE message_source = $1
  AND message_id = $2
  AND published_at IS NULL
  AND quarantined_at IS NULL
  AND lease_owner = $3
  AND lease_expires_at > clock_timestamp()
"#;

impl PostgresOutbox {
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
                CLAIM_BATCH_SQL,
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
                "UPDATE edgeagent_message_outbox SET published_at = clock_timestamp(), lease_owner = NULL, lease_expires_at = NULL, last_failure_code = NULL WHERE message_source = $1 AND message_id = $2 AND published_at IS NULL AND quarantined_at IS NULL AND lease_owner = $3 AND lease_expires_at > clock_timestamp()",
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
                RELEASE_FOR_RETRY_SQL,
                &[
                    &message_source,
                    &message_id,
                    &lease_owner,
                    &retry_milliseconds,
                    &failure_code,
                ],
            )
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated)
    }

    /// Move a leased message into retained terminal quarantine.
    ///
    /// Quarantined records are excluded from relay claims until a separately
    /// authorized operator-replay workflow changes their state.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument, storage, or lost-lease error.
    pub async fn quarantine(
        &self,
        transaction: &Transaction<'_>,
        message_source: &str,
        message_id: &str,
        lease_owner: &str,
        reason: &str,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        validate_token("quarantine_reason", reason, MAX_FAILURE_CODE_BYTES)?;
        let updated = transaction
            .execute(
                "UPDATE edgeagent_message_outbox SET quarantined_at = clock_timestamp(), quarantine_reason = $4, lease_owner = NULL, lease_expires_at = NULL, last_failure_code = $4 WHERE message_source = $1 AND message_id = $2 AND published_at IS NULL AND quarantined_at IS NULL AND lease_owner = $3 AND lease_expires_at > clock_timestamp()",
                &[&message_source, &message_id, &lease_owner, &reason],
            )
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated)
    }
}
fn claimed_message(row: Row) -> Result<ClaimedMessage, OutboxError> {
    let attempt: i32 = row.try_get(5).map_err(OutboxError::storage)?;
    ClaimedMessage::new(
        row.try_get(0).map_err(OutboxError::storage)?,
        row.try_get(1).map_err(OutboxError::storage)?,
        row.try_get(2).map_err(OutboxError::storage)?,
        row.try_get(3).map_err(OutboxError::storage)?,
        row.try_get(4).map_err(OutboxError::storage)?,
        u32::try_from(attempt).map_err(|_| OutboxError::storage_invariant())?,
    )
    .map_err(|_| OutboxError::storage_invariant())
}

fn require_updated(updated: u64) -> Result<(), OutboxError> {
    if updated == 1 {
        Ok(())
    } else {
        Err(OutboxError::lease_lost())
    }
}
