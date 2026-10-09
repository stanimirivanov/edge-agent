//! Claim, publish, retry, and quarantine transitions under expiring leases.

use crate::validation::{
    MAX_BATCH_SIZE, MAX_FAILURE_CODE_BYTES, MAX_LEASE_OWNER_BYTES, duration_milliseconds,
    validate_token,
};
use crate::{ClaimedMessage, OutboxError, PostgresOutbox};
use edgeagent_messaging::{FailureCode, LeaseDuration, LeaseGeneration, OutboxRetryDelay};
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
struct StoredClaim {
    message_source: String,
    message_id: String,
    message_type: String,
    transport_subject: String,
    envelope: Vec<u8>,
    attempt_count: i32,
    lease_generation: i64,
}

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
    attempt_count = outbox.attempt_count + 1,
    lease_generation = outbox.lease_generation + 1
FROM candidates
WHERE outbox.message_source = candidates.message_source
  AND outbox.message_id = candidates.message_id
RETURNING outbox.message_source,
          outbox.message_id,
          outbox.message_type,
          outbox.transport_subject,
          outbox.envelope,
          outbox.attempt_count,
          outbox.lease_generation
"#;
pub(super) const RELEASE_FOR_RETRY_SQL: &str = r#"
UPDATE edgeagent_message_outbox
SET available_at = clock_timestamp() + ($5::BIGINT * INTERVAL '1 millisecond'),
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = $6
WHERE message_source = $1
  AND message_id = $2
  AND published_at IS NULL
  AND quarantined_at IS NULL
  AND lease_owner = $3
  AND lease_generation = $4
  AND lease_expires_at > clock_timestamp()
"#;
const MARK_PUBLISHED_SQL: &str = r"
UPDATE edgeagent_message_outbox
SET published_at = clock_timestamp(),
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = NULL
WHERE message_source = $1
  AND message_id = $2
  AND published_at IS NULL
  AND quarantined_at IS NULL
  AND lease_owner = $3
  AND lease_generation = $4
  AND lease_expires_at > clock_timestamp()
";
const QUARANTINE_SQL: &str = r"
UPDATE edgeagent_message_outbox
SET quarantined_at = clock_timestamp(),
    quarantine_reason = $5,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = $5
WHERE message_source = $1
  AND message_id = $2
  AND published_at IS NULL
  AND quarantined_at IS NULL
  AND lease_owner = $3
  AND lease_generation = $4
  AND lease_expires_at > clock_timestamp()
";

impl PostgresOutbox {
    /// Lease the next available records using PostgreSQL `SKIP LOCKED`.
    ///
    /// Concurrent relays receive disjoint records. Expired leases become
    /// eligible again, incrementing the attempt count on the next claim.
    /// The lease arrives prevalidated and is persisted in whole milliseconds,
    /// flooring any fractional millisecond.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument, storage, or storage-invariant error.
    pub async fn claim_batch(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        lease_owner: &str,
        batch_size: u16,
        lease_duration: LeaseDuration,
    ) -> Result<Vec<ClaimedMessage>, OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        if batch_size == 0 || batch_size > MAX_BATCH_SIZE {
            return Err(OutboxError::invalid_argument(
                "batch_size must be between 1 and 1000",
            ));
        }
        let lease_milliseconds = duration_milliseconds(lease_duration.get())?;
        let rows = sqlx::query_as::<_, StoredClaim>(CLAIM_BATCH_SQL)
            .bind(i64::from(batch_size))
            .bind(lease_owner)
            .bind(lease_milliseconds)
            .fetch_all(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?;
        rows.into_iter().map(claimed_message).collect()
    }

    /// Mark a leased message as durably published and retain its audit record.
    ///
    /// # Errors
    ///
    /// Returns `LeaseLost` if the exact claim is absent, expired, or superseded.
    pub async fn mark_published(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let updated = sqlx::query(MARK_PUBLISHED_SQL)
            .bind(claim.message_source())
            .bind(claim.message_id())
            .bind(lease_owner)
            .bind(lease_generation)
            .execute(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated.rows_affected())
    }

    /// Release a leased message after a classified failure and schedule retry.
    ///
    /// The delay arrives prevalidated and is persisted in whole milliseconds.
    /// Zero and submillisecond delays make the message immediately eligible.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument, storage, or lost-lease error.
    pub async fn release_for_retry(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        validate_token(
            "failure_code",
            failure_code.as_str(),
            MAX_FAILURE_CODE_BYTES,
        )?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let retry_milliseconds = duration_milliseconds(retry_after.get())?;
        let updated = sqlx::query(RELEASE_FOR_RETRY_SQL)
            .bind(claim.message_source())
            .bind(claim.message_id())
            .bind(lease_owner)
            .bind(lease_generation)
            .bind(retry_milliseconds)
            .bind(failure_code.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated.rows_affected())
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
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
        reason: FailureCode,
    ) -> Result<(), OutboxError> {
        validate_token("lease_owner", lease_owner, MAX_LEASE_OWNER_BYTES)?;
        validate_token("quarantine_reason", reason.as_str(), MAX_FAILURE_CODE_BYTES)?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let updated = sqlx::query(QUARANTINE_SQL)
            .bind(claim.message_source())
            .bind(claim.message_id())
            .bind(lease_owner)
            .bind(lease_generation)
            .bind(reason.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(OutboxError::storage)?;
        require_updated(updated.rows_affected())
    }
}
fn claimed_message(row: StoredClaim) -> Result<ClaimedMessage, OutboxError> {
    let generation = u64::try_from(row.lease_generation)
        .ok()
        .and_then(|generation| LeaseGeneration::new(generation).ok())
        .ok_or_else(OutboxError::storage_invariant)?;
    ClaimedMessage::new(
        row.message_source,
        row.message_id,
        row.message_type,
        row.transport_subject,
        row.envelope,
        u32::try_from(row.attempt_count).map_err(|_| OutboxError::storage_invariant())?,
        generation,
    )
    .map_err(|_| OutboxError::storage_invariant())
}

fn sql_lease_generation(generation: LeaseGeneration) -> Result<i64, OutboxError> {
    i64::try_from(generation.get()).map_err(|_| {
        OutboxError::invalid_argument("lease_generation exceeds PostgreSQL bigint range")
    })
}

fn require_updated(updated: u64) -> Result<(), OutboxError> {
    if updated == 1 {
        Ok(())
    } else {
        Err(OutboxError::lease_lost())
    }
}
