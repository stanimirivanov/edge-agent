use crate::replay::validate_target;
use crate::validation::{
    MAX_FAILURE_CODE_BYTES, MAX_IDENTIFIER_BYTES, MAX_LEASE_OWNER_BYTES, MAX_MESSAGE_SOURCE_BYTES,
    MAX_OPERATOR_ID_BYTES, duration_milliseconds, validate_batch_size, validate_lease_owner,
    validate_token,
};
use crate::{OutboxErrorKind, PostgresOutbox, ReplayRequest};
use edgeagent_contracts::{MessageEnvelope, MessageMetadata};
use edgeagent_messaging::{FailureCode, LeaseDuration, OutboxRetryDelay};

use std::time::Duration;

#[test]
fn worker_token_and_batch_size_are_bounded_before_database_work() {
    assert!(validate_lease_owner("relay_01").is_ok());
    assert_eq!(
        validate_lease_owner("Relay 01")
            .err()
            .map(|error| error.kind()),
        Some(OutboxErrorKind::InvalidArgument)
    );
    for (size, accepted) in [(0, false), (1, true), (1_000, true), (1_001, false)] {
        assert_eq!(
            validate_batch_size(size).is_ok(),
            accepted,
            "batch size {size}"
        );
    }
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
    assert_eq!(
        PostgresOutbox::MIGRATIONS,
        [
            PostgresOutbox::MIGRATION_SQL,
            PostgresOutbox::QUARANTINE_MIGRATION_SQL,
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            PostgresOutbox::LEASE_GENERATION_MIGRATION_SQL,
        ]
    );
    assert!(PostgresOutbox::QUARANTINE_MIGRATION_SQL.contains("quarantined_at TIMESTAMPTZ"));
    assert!(PostgresOutbox::REPLAY_MIGRATION_SQL.contains("edgeagent_message_outbox_replay_audit"));
    assert!(PostgresOutbox::LEASE_GENERATION_MIGRATION_SQL.contains("lease_generation BIGINT"));
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
fn adapter_tokens_keep_explicit_diagnostics_and_storage_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    for length in [0, 1, 128, 129] {
        assert_eq!(
            validate_lease_owner(&"a".repeat(length)).is_ok(),
            (1..=128).contains(&length)
        );
    }
    for value in ["", "UPPER", "two words", "é", &"a".repeat(65)] {
        let error = validate_token(value, MAX_FAILURE_CODE_BYTES, "explicit caller diagnostic")
            .err()
            .ok_or("invalid token must fail")?;
        assert_eq!(error.kind(), OutboxErrorKind::InvalidArgument);
        assert_eq!(
            error.to_string(),
            "outbox argument is invalid: explicit caller diagnostic"
        );
    }
    for length in [1, 64, 65] {
        let code = "a".repeat(length);
        let static_code: &'static str = match length {
            1 => "a",
            64 => concat!(
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa"
            ),
            _ => concat!(
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaaa"
            ),
        };
        assert_eq!(
            std::panic::catch_unwind(|| FailureCode::from_static(static_code)).is_ok(),
            length <= MAX_FAILURE_CODE_BYTES
        );
        assert_eq!(
            validate_token(&code, MAX_FAILURE_CODE_BYTES, "failure code bound").is_ok(),
            length <= 64
        );
    }
    Ok(())
}

fn envelope_metadata(id: String, source: String) -> MessageMetadata {
    MessageMetadata {
        id,
        source,
        message_type: "com.edgeagent.research.request-received.v1".into(),
        subject: "request/request-01".into(),
        time: "2026-09-26T00:00:00Z".into(),
        data_schema: "urn:edgeagent:schema:request-received:v1".into(),
        correlation_id: "correlation-01".into(),
        causation_id: "command-01".into(),
        idempotency_key: "request-01".into(),
        partition_key: "request/request-01".into(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
        trace_state: None,
    }
}

#[test]
fn replay_targets_accept_contract_identity_bounds_without_revalidating_enqueues() {
    for length in [0, 1, 128, 129] {
        let id = "a".repeat(length);
        let source = "urn:edgeagent:test";
        let accepted = (1..=128).contains(&length);
        assert_eq!(
            MessageEnvelope::from_payload(envelope_metadata(id.clone(), source.into()), &())
                .is_ok(),
            accepted
        );
        assert_eq!(validate_target(source, &id).is_ok(), accepted);
    }
    for length in [511, 512, 513] {
        let source = format!("urn:test:{}", "a".repeat(length - 9));
        assert_eq!(source.len(), length);
        assert_eq!(
            MessageEnvelope::from_payload(envelope_metadata("id".into(), source.clone()), &())
                .is_ok(),
            length <= 512
        );
        assert_eq!(validate_target(&source, "id").is_ok(), length <= 512);
    }
    for id in ["a-Z_0.:b", "two words", "é", "a/b"] {
        let envelope = MessageEnvelope::from_payload(
            envelope_metadata(id.into(), "urn:test:source".into()),
            &(),
        );
        assert_eq!(
            validate_target("urn:test:source", id).is_ok(),
            envelope.is_ok()
        );
    }
}

#[test]
fn replay_authorization_inclusive_and_adjacent_bounds_are_exercised() {
    for length in [0, 1, 128, 129] {
        assert_eq!(
            ReplayRequest::new(&"a".repeat(length), "operator", "reason").is_ok(),
            (1..=128).contains(&length)
        );
    }
    for length in [0, 1, 256, 257] {
        assert_eq!(
            ReplayRequest::new("request", &"a".repeat(length), "reason").is_ok(),
            (1..=256).contains(&length)
        );
    }
    for length in [0, 1, 64, 65] {
        assert_eq!(
            ReplayRequest::new("request", "operator", &"a".repeat(length)).is_ok(),
            (1..=64).contains(&length)
        );
    }
    for actor in ["two words", "é", "line\nfeed"] {
        assert!(ReplayRequest::new("request", actor, "reason").is_err());
    }
}

#[test]
fn adapter_limits_match_persisted_column_widths() -> Result<(), Box<dyn std::error::Error>> {
    // Read column/type tokens, not SQL fragments or query formatting. Runtime
    // conformance owns fencing/replay behavior; these assertions own DDL widths.
    fn varchar_width(sql: &str, column: &str) -> Option<usize> {
        let normalized = sql.replace(['(', ')', ','], " ").to_ascii_lowercase();
        let tokens: Vec<_> = normalized.split_whitespace().collect();
        tokens.windows(3).find_map(|tokens| {
            if tokens[0] == column && tokens[1] == "varchar" {
                tokens[2].parse().ok()
            } else {
                None
            }
        })
    }
    for (sql, column, bound) in [
        (
            PostgresOutbox::MIGRATION_SQL,
            "message_id",
            MAX_IDENTIFIER_BYTES,
        ),
        (
            PostgresOutbox::MIGRATION_SQL,
            "lease_owner",
            MAX_LEASE_OWNER_BYTES,
        ),
        (
            PostgresOutbox::MIGRATION_SQL,
            "last_failure_code",
            MAX_FAILURE_CODE_BYTES,
        ),
        (
            PostgresOutbox::QUARANTINE_MIGRATION_SQL,
            "quarantine_reason",
            MAX_FAILURE_CODE_BYTES,
        ),
        (
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            "replay_request_id",
            MAX_IDENTIFIER_BYTES,
        ),
        (
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            "message_id",
            MAX_IDENTIFIER_BYTES,
        ),
        (
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            "requested_by",
            MAX_OPERATOR_ID_BYTES,
        ),
        (
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            "reason",
            MAX_FAILURE_CODE_BYTES,
        ),
        (
            PostgresOutbox::REPLAY_MIGRATION_SQL,
            "prior_quarantine_reason",
            MAX_FAILURE_CODE_BYTES,
        ),
    ] {
        assert_eq!(
            varchar_width(sql, column).ok_or("missing VARCHAR column")?,
            bound,
            "{column}"
        );
    }
    // Sources deliberately use TEXT in PostgreSQL, but replay remains bounded
    // by the validated envelope contract rather than the database's text limit.
    assert_eq!(MAX_MESSAGE_SOURCE_BYTES, 512);
    Ok(())
}
