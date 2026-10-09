use crate::leasing::{CLAIM_BATCH_SQL, RELEASE_FOR_RETRY_SQL};
use crate::replay::REPLAY_QUARANTINED_SQL;
use crate::validation::{MAX_BATCH_SIZE, duration_milliseconds, validate_token};
use crate::{OutboxErrorKind, PostgresOutbox, ReplayRequest};
use edgeagent_messaging::{LeaseDuration, OutboxRetryDelay};

use std::time::Duration;

#[test]
fn worker_token_and_batch_size_are_bounded_before_database_work() {
    assert!(validate_token("lease_owner", "relay_01", 128).is_ok());
    assert_eq!(
        validate_token("lease_owner", "Relay 01", 128)
            .err()
            .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
    assert_eq!(MAX_BATCH_SIZE, 1_000);
}

#[test]
fn bounded_timing_values_preserve_integer_millisecond_conversion()
-> Result<(), Box<dyn std::error::Error>> {
    for duration in [LeaseDuration::MIN, LeaseDuration::MAX] {
        let lease = LeaseDuration::new(duration)?;
        assert_eq!(
            duration_milliseconds(lease.get())?,
            i64::try_from(duration.as_millis())?
        );
    }
    for duration in [OutboxRetryDelay::MIN, OutboxRetryDelay::MAX] {
        let retry = OutboxRetryDelay::new(duration)?;
        assert_eq!(
            duration_milliseconds(retry.get())?,
            i64::try_from(duration.as_millis())?
        );
    }
    assert_eq!(duration_milliseconds(OutboxRetryDelay::IMMEDIATE.get())?, 0);
    let submillisecond_retry = OutboxRetryDelay::new(Duration::from_nanos(999_999))?;
    assert_eq!(duration_milliseconds(submillisecond_retry.get())?, 0);
    let fractional_lease = LeaseDuration::new(Duration::from_nanos(1_999_999))?;
    assert_eq!(duration_milliseconds(fractional_lease.get())?, 1);
    Ok(())
}

#[test]
fn millisecond_conversion_rejects_postgres_integer_overflow() {
    assert_eq!(
        duration_milliseconds(Duration::MAX)
            .err()
            .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
}

#[test]
fn migration_uses_exact_bytes_and_source_scoped_identity() {
    assert!(PostgresOutbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
    assert!(PostgresOutbox::MIGRATION_SQL.contains("PRIMARY KEY (message_source, message_id)"));
    assert_eq!(PostgresOutbox::MIGRATIONS.len(), 4);
    assert!(PostgresOutbox::QUARANTINE_MIGRATION_SQL.contains("quarantined_at TIMESTAMPTZ"));
    assert!(PostgresOutbox::REPLAY_MIGRATION_SQL.contains("edgeagent_message_outbox_replay_audit"));
    assert!(PostgresOutbox::LEASE_GENERATION_MIGRATION_SQL.contains("lease_generation BIGINT"));
    assert!(CLAIM_BATCH_SQL.contains("quarantined_at IS NULL"));
    assert!(CLAIM_BATCH_SQL.contains("lease_generation = outbox.lease_generation + 1"));
    assert!(RELEASE_FOR_RETRY_SQL.contains("lease_generation = $4"));
    assert!(REPLAY_QUARANTINED_SQL.contains("attempt_count = 0"));
    assert!(!REPLAY_QUARANTINED_SQL.contains("lease_generation ="));
}

#[test]
fn replay_authorization_evidence_is_bounded_before_database_work() {
    assert!(
        ReplayRequest::new(
            "replay-request-01",
            "urn:edgeagent:operator:alice",
            "configuration_remediated",
        )
        .is_ok()
    );
    assert_eq!(
        ReplayRequest::new(
            "replay request 01",
            "urn:edgeagent:operator:alice",
            "configuration_remediated",
        )
        .err()
        .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
    assert_eq!(
        ReplayRequest::new(
            "replay-request-01",
            "urn:edgeagent:operator:alice",
            "Configuration remediated",
        )
        .err()
        .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
}

#[test]
fn interval_parameters_are_prepared_as_integer_milliseconds() {
    assert!(CLAIM_BATCH_SQL.contains("$3::BIGINT * INTERVAL '1 millisecond'"));
    assert!(RELEASE_FOR_RETRY_SQL.contains("$5::BIGINT * INTERVAL '1 millisecond'"));
}
