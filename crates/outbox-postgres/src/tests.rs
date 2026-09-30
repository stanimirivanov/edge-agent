use crate::leasing::{CLAIM_BATCH_SQL, RELEASE_FOR_RETRY_SQL};
use crate::replay::REPLAY_QUARANTINED_SQL;
use crate::validation::{
    MAX_BATCH_SIZE, MAX_LEASE_DURATION, duration_milliseconds, validate_token,
};
use crate::{OutboxErrorKind, PostgresOutbox, ReplayRequest};

use std::time::Duration;

#[test]
fn lease_arguments_are_bounded_before_database_work() {
    assert!(validate_token("lease_owner", "relay_01", 128).is_ok());
    assert_eq!(
        validate_token("lease_owner", "Relay 01", 128)
            .err()
            .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
    assert_eq!(MAX_BATCH_SIZE, 1_000);
    assert_eq!(
        duration_milliseconds(
            "lease_duration",
            MAX_LEASE_DURATION + Duration::from_millis(1),
            Duration::from_millis(1),
            MAX_LEASE_DURATION,
        )
        .err()
        .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
}

#[test]
fn migration_uses_exact_bytes_and_source_scoped_identity() {
    assert!(PostgresOutbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
    assert!(PostgresOutbox::MIGRATION_SQL.contains("PRIMARY KEY (message_source, message_id)"));
    assert_eq!(PostgresOutbox::MIGRATIONS.len(), 3);
    assert!(PostgresOutbox::QUARANTINE_MIGRATION_SQL.contains("quarantined_at TIMESTAMPTZ"));
    assert!(PostgresOutbox::REPLAY_MIGRATION_SQL.contains("edgeagent_message_outbox_replay_audit"));
    assert!(CLAIM_BATCH_SQL.contains("quarantined_at IS NULL"));
    assert!(REPLAY_QUARANTINED_SQL.contains("attempt_count = 0"));
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
    assert!(RELEASE_FOR_RETRY_SQL.contains("$4::BIGINT * INTERVAL '1 millisecond'"));
}
