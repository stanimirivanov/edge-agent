use crate::replay::AUTHORIZE_QUARANTINE_REPLAY_SQL;
use crate::validation::validate_consumer_name;
use crate::{InboxErrorKind, PostgresInbox, QuarantineEvidence, ReplayRequest};
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;

#[test]
fn consumer_names_are_bounded_stable_tokens() {
    assert!(validate_consumer_name("execution_simulator_v1").is_ok());
    assert_eq!(
        validate_consumer_name("Execution Simulator")
            .err()
            .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidConsumerName)
    );
    assert_eq!(
        validate_consumer_name(&"a".repeat(129))
            .err()
            .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidConsumerName)
    );
}

#[test]
fn migration_scopes_identity_by_consumer_and_stores_exact_bytes() {
    assert!(PostgresInbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
    assert!(
        PostgresInbox::MIGRATION_SQL
            .contains("PRIMARY KEY (consumer_name, message_source, message_id)")
    );
    assert_eq!(PostgresInbox::MIGRATIONS.len(), 3);
    assert!(
        PostgresInbox::QUARANTINE_MIGRATION_SQL
            .contains("PRIMARY KEY (consumer_name, delivery_key)")
    );
    assert!(PostgresInbox::QUARANTINE_MIGRATION_SQL.contains("octet_length(payload) <= 262144"));
    assert!(
        PostgresInbox::REPLAY_MIGRATION_SQL.contains("edgeagent_message_quarantine_replay_audit")
    );
    assert!(AUTHORIZE_QUARANTINE_REPLAY_SQL.contains("FOR UPDATE"));
}

#[test]
fn quarantine_evidence_is_bounded_before_database_work() {
    assert!(
        QuarantineEvidence::new(
            "6:ORDERS:41",
            "edgeagent.command.execution.submit-dry-run-order.v1",
            2,
            b"not-json",
            "envelope_invalid",
        )
        .is_ok()
    );
    assert_eq!(
        QuarantineEvidence::new(
            "6:ORDERS:41",
            "edgeagent.command.execution.submit-dry-run-order.v1",
            2,
            b"not-json",
            "Envelope Invalid",
        )
        .err()
        .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidQuarantineEvidence)
    );
    let oversized = vec![0_u8; MAX_PORTABLE_MESSAGE_BYTES + 1];
    assert_eq!(
        QuarantineEvidence::new(
            "6:ORDERS:41",
            "edgeagent.command.execution.submit-dry-run-order.v1",
            2,
            &oversized,
            "envelope_invalid",
        )
        .err()
        .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidQuarantineEvidence)
    );
}

#[test]
fn replay_authorization_evidence_is_bounded_before_database_work() {
    assert!(
        ReplayRequest::new(
            "replay-request-01",
            "urn:edgeagent:operator:alice",
            "consumer_remediated",
        )
        .is_ok()
    );
    assert_eq!(
        ReplayRequest::new(
            "replay request 01",
            "urn:edgeagent:operator:alice",
            "consumer_remediated",
        )
        .err()
        .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidReplayRequest)
    );
    assert_eq!(
        ReplayRequest::new(
            "replay-request-01",
            "urn:edgeagent:operator:alice",
            "Consumer remediated",
        )
        .err()
        .map(|error| error.kind()),
        Some(InboxErrorKind::InvalidReplayRequest)
    );
}
