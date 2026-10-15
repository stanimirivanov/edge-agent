//! Claim, publish, retry, and quarantine transitions under expiring leases.

use crate::validation::{
    MAX_FAILURE_CODE_BYTES, duration_milliseconds, validate_batch_size, validate_lease_owner,
    validate_token,
};
use crate::{ClaimedMessage, OutboxError, PostgresOutbox};
use edgeagent_messaging::{FailureCode, LeaseDuration, LeaseGeneration, OutboxRetryDelay};
use sqlx::{Postgres, Transaction};

struct StoredClaim {
    message_source: String,
    message_id: String,
    message_type: String,
    transport_subject: String,
    envelope: Vec<u8>,
    attempt_count: i32,
    lease_generation: i64,
}

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
        validate_lease_owner(lease_owner)?;
        validate_batch_size(batch_size)?;
        let lease_milliseconds = duration_milliseconds(lease_duration.get())?;
        let rows = sqlx::query_file_as!(
            StoredClaim,
            "queries/claim_batch.sql",
            i64::from(batch_size),
            lease_owner,
            lease_milliseconds,
        )
        .fetch_all(&mut **transaction)
        .await
        .map_err(OutboxError::storage)?;
        rows.into_iter().map(claimed_message).collect()
    }

    /// Mark a leased message as durably published and retain its audit record.
    ///
    /// # Errors
    ///
    /// Returns `LeaseLost` if the exact claim is absent, expired, or superseded;
    /// `InvalidArgument` for an invalid owner or unsupported generation range;
    /// or a classified `Storage` or `StorageInvariant` database failure.
    pub async fn mark_published(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
    ) -> Result<(), OutboxError> {
        validate_lease_owner(lease_owner)?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let updated = sqlx::query_file!(
            "queries/mark_published.sql",
            claim.message_source(),
            claim.message_id(),
            lease_owner,
            lease_generation,
        )
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
    /// Returns `InvalidArgument`, `Storage`, `StorageInvariant`, or `LeaseLost`.
    pub async fn release_for_retry(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> Result<(), OutboxError> {
        validate_lease_owner(lease_owner)?;
        validate_token(
            failure_code.as_str(),
            MAX_FAILURE_CODE_BYTES,
            "failure_code must contain 1 to 64 lowercase ASCII token bytes",
        )?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let retry_milliseconds = duration_milliseconds(retry_after.get())?;
        let updated = sqlx::query_file!(
            "queries/release_for_retry.sql",
            claim.message_source(),
            claim.message_id(),
            lease_owner,
            lease_generation,
            retry_milliseconds,
            failure_code.as_str(),
        )
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
    /// Returns `InvalidArgument`, `Storage`, `StorageInvariant`, or `LeaseLost`.
    pub async fn quarantine(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claim: &ClaimedMessage,
        lease_owner: &str,
        reason: FailureCode,
    ) -> Result<(), OutboxError> {
        validate_lease_owner(lease_owner)?;
        validate_token(
            reason.as_str(),
            MAX_FAILURE_CODE_BYTES,
            "quarantine_reason must contain 1 to 64 lowercase ASCII token bytes",
        )?;
        let lease_generation = sql_lease_generation(claim.lease_generation())?;
        let updated = sqlx::query_file!(
            "queries/quarantine.sql",
            claim.message_source(),
            claim.message_id(),
            lease_owner,
            lease_generation,
            reason.as_str(),
        )
        .execute(&mut **transaction)
        .await
        .map_err(OutboxError::storage)?;
        require_updated(updated.rows_affected())
    }
}
fn claimed_message(row: StoredClaim) -> Result<ClaimedMessage, OutboxError> {
    let generation = u64::try_from(row.lease_generation).map_err(|source| {
        OutboxError::storage_invariant_with_source(
            "stored lease_generation is outside the unsigned integer range",
            source,
        )
    })?;
    let generation = LeaseGeneration::new(generation).map_err(|source| {
        OutboxError::storage_invariant_with_source(
            "stored lease_generation is not a positive claim generation",
            source,
        )
    })?;
    let attempt = u32::try_from(row.attempt_count).map_err(|source| {
        OutboxError::storage_invariant_with_source(
            "stored attempt_count is outside the unsigned integer range",
            source,
        )
    })?;
    ClaimedMessage::new(
        row.message_source,
        row.message_id,
        row.message_type,
        row.transport_subject,
        row.envelope,
        attempt,
        generation,
    )
    .map_err(|source| {
        OutboxError::storage_invariant_with_source(
            "stored claim violates portable claim invariants",
            source,
        )
    })
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

#[cfg(test)]
mod tests;
