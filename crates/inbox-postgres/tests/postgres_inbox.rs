//! Opt-in conformance test against the checked-in local PostgreSQL profile.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_postgres::{
    DeliveryDisposition, InboxErrorKind, PostgresInbox, QuarantineDisposition, QuarantineEvidence,
    ReplayDisposition, ReplayRequest,
};
use serde_json::json;
use sqlx::{Connection, PgConnection, Postgres, Row, Transaction};
use std::env;
use std::error::Error;
use std::process;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn metadata(message_id: &str) -> MessageMetadata {
    MessageMetadata {
        id: message_id.to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: COMMAND.message_type().to_owned(),
        subject: "order/inbox-order-01".to_owned(),
        time: "2026-09-27T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema().to_owned(),
        correlation_id: "inbox-correlation-01".to_owned(),
        causation_id: "inbox-request-01".to_owned(),
        idempotency_key: "inbox-order-01".to_owned(),
        partition_key: "order/inbox-order-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

async fn apply_effect(
    transaction: &mut Transaction<'_, Postgres>,
    consumer_name: &str,
    envelope: &MessageEnvelope,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO inbox_effects (consumer_name, message_source, message_id) VALUES ($1, $2, $3)",
    )
    .bind(consumer_name)
    .bind(envelope.source())
    .bind(envelope.id())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn inbox_and_quarantine_preserve_consumer_invariants() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_inbox_test_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresInbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }
    sqlx::raw_sql(
            "CREATE TABLE inbox_effects (consumer_name VARCHAR(128) NOT NULL, message_source TEXT NOT NULL, message_id VARCHAR(128) NOT NULL, PRIMARY KEY (consumer_name, message_source, message_id))",
        )
        .execute(&mut client)
        .await?;

    let inbox = PostgresInbox;
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let envelope = COMMAND.build(metadata("inbox-message-01"), &json!({"quantity": 1}))?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &envelope
            )
            .await?,
        DeliveryDisposition::FirstDelivery
    );
    apply_effect(&mut transaction, "execution_simulator_v1", &envelope).await?;
    transaction.rollback().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &envelope
            )
            .await?,
        DeliveryDisposition::FirstDelivery
    );
    apply_effect(&mut transaction, "execution_simulator_v1", &envelope).await?;
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &envelope
            )
            .await?,
        DeliveryDisposition::Duplicate
    );
    transaction.commit().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_effects")
        .fetch_one(&mut client)
        .await?;
    assert_eq!(count, 1);

    let conflicting = COMMAND.build(metadata("inbox-message-01"), &json!({"quantity": 2}))?;
    let mut transaction = client.begin().await?;
    let conflict = inbox
        .record_delivery(
            &mut transaction,
            "execution_simulator_v1",
            &registry,
            &conflicting,
        )
        .await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(InboxErrorKind::MessageIdentityConflict)
    );
    transaction.rollback().await?;

    // A large envelope must still compare exactly without returning stored
    // BYTEA to the adapter after the insert loses its identity race.
    let large_envelope = COMMAND.build(
        metadata("inbox-message-large"),
        &json!({"blob": "x".repeat(128 * 1024)}),
    )?;
    let conflicting_large_envelope = COMMAND.build(
        metadata("inbox-message-large"),
        &json!({"blob": format!("{}y", "x".repeat(128 * 1024 - 1))}),
    )?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &large_envelope,
            )
            .await?,
        DeliveryDisposition::FirstDelivery
    );
    transaction.commit().await?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &large_envelope,
            )
            .await?,
        DeliveryDisposition::Duplicate
    );
    assert_eq!(
        inbox
            .record_delivery(
                &mut transaction,
                "execution_simulator_v1",
                &registry,
                &conflicting_large_envelope,
            )
            .await
            .err()
            .map(|error| error.kind()),
        Some(InboxErrorKind::MessageIdentityConflict)
    );
    transaction.rollback().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .record_delivery(&mut transaction, "audit_projector_v1", &registry, &envelope)
            .await?,
        DeliveryDisposition::FirstDelivery
    );
    apply_effect(&mut transaction, "audit_projector_v1", &envelope).await?;
    transaction.commit().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_effects")
        .fetch_one(&mut client)
        .await?;
    assert_eq!(count, 2);

    let mut unsupported_metadata = metadata("inbox-message-02");
    unsupported_metadata.message_type =
        "com.edgeagent.execution.submit-dry-run-order.v2".to_owned();
    let unsupported = MessageEnvelope::from_payload(unsupported_metadata, &json!({}))?;
    let mut transaction = client.begin().await?;
    let unsupported_result = inbox
        .record_delivery(
            &mut transaction,
            "execution_simulator_v1",
            &registry,
            &unsupported,
        )
        .await;
    assert_eq!(
        unsupported_result.err().map(|error| error.kind()),
        Some(InboxErrorKind::Contract)
    );
    transaction.rollback().await?;

    let poison_payload = b"{not-json";
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    "18:EDGEAGENT_COMMANDS:7",
                    "edgeagent.command.execution.submit-dry-run-order.v1",
                    1,
                    poison_payload,
                    "envelope_invalid",
                )?,
            )
            .await?,
        QuarantineDisposition::Inserted
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    "18:EDGEAGENT_COMMANDS:7",
                    "edgeagent.command.execution.submit-dry-run-order.v1",
                    2,
                    poison_payload,
                    "envelope_invalid",
                )?,
            )
            .await?,
        QuarantineDisposition::AlreadyPresent
    );
    transaction.commit().await?;
    let quarantine = sqlx::query(
        "SELECT count(*), max(last_delivery_attempt) FROM edgeagent_message_quarantine",
    )
    .fetch_one(&mut client)
    .await?;
    assert_eq!(quarantine.try_get::<i64, _>(0)?, 1);
    assert_eq!(quarantine.try_get::<Option<i64>, _>(1)?, Some(2));

    let mut transaction = client.begin().await?;
    let quarantine_conflict = inbox
        .quarantine_delivery(
            &mut transaction,
            "execution_simulator_v1",
            QuarantineEvidence::new(
                "18:EDGEAGENT_COMMANDS:7",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                3,
                b"different-invalid-bytes",
                "envelope_invalid",
            )?,
        )
        .await;
    assert_eq!(
        quarantine_conflict.err().map(|error| error.kind()),
        Some(InboxErrorKind::QuarantineIdentityConflict)
    );
    for evidence in [
        QuarantineEvidence::new(
            "18:EDGEAGENT_COMMANDS:7",
            "edgeagent.command.execution.other.v1",
            3,
            poison_payload,
            "envelope_invalid",
        )?,
        QuarantineEvidence::new(
            "18:EDGEAGENT_COMMANDS:7",
            "edgeagent.command.execution.submit-dry-run-order.v1",
            3,
            poison_payload,
            "other_failure",
        )?,
    ] {
        assert_eq!(
            inbox
                .quarantine_delivery(&mut transaction, "execution_simulator_v1", evidence)
                .await
                .err()
                .map(|error| error.kind()),
            Some(InboxErrorKind::QuarantineIdentityConflict)
        );
    }
    transaction.rollback().await?;

    // A late redelivery can reach the portable u32 attempt maximum without
    // changing the immutable evidence retained on the first observation.
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    "18:EDGEAGENT_COMMANDS:7",
                    "edgeagent.command.execution.submit-dry-run-order.v1",
                    u32::MAX,
                    poison_payload,
                    "envelope_invalid",
                )?,
            )
            .await?,
        QuarantineDisposition::AlreadyPresent
    );
    transaction.commit().await?;

    let maximum_attempt_key = "18:EDGEAGENT_COMMANDS:max-attempt";
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    maximum_attempt_key,
                    "edgeagent.command.execution.submit-dry-run-order.v1",
                    u32::MAX,
                    poison_payload,
                    "envelope_invalid",
                )?,
            )
            .await?,
        QuarantineDisposition::Inserted
    );
    transaction.commit().await?;
    let maximum_attempts = sqlx::query(
        "SELECT first_delivery_attempt, last_delivery_attempt FROM edgeagent_message_quarantine WHERE consumer_name = $1 AND delivery_key = $2",
    )
    .bind("execution_simulator_v1")
    .bind(maximum_attempt_key)
    .fetch_one(&mut client)
    .await?;
    assert_eq!(maximum_attempts.try_get::<i64, _>(0)?, i64::from(u32::MAX));
    assert_eq!(maximum_attempts.try_get::<i64, _>(1)?, i64::from(u32::MAX));

    let large_payload = vec![b'x'; 256 * 1024];
    let mut conflicting_large_payload = large_payload.clone();
    let last_byte_index = conflicting_large_payload.len() - 1;
    conflicting_large_payload[last_byte_index] = b'y';
    let large_key = "18:EDGEAGENT_COMMANDS:large";
    let subject = "edgeagent.command.execution.submit-dry-run-order.v1";
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(large_key, subject, 1, &large_payload, "envelope_invalid")?,
            )
            .await?,
        QuarantineDisposition::Inserted
    );
    transaction.commit().await?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(large_key, subject, 2, &large_payload, "envelope_invalid")?,
            )
            .await?,
        QuarantineDisposition::AlreadyPresent
    );
    assert_eq!(
        inbox
            .quarantine_delivery(
                &mut transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    large_key,
                    subject,
                    3,
                    &conflicting_large_payload,
                    "envelope_invalid",
                )?,
            )
            .await
            .err()
            .map(|error| error.kind()),
        Some(InboxErrorKind::QuarantineIdentityConflict)
    );
    transaction.commit().await?;
    let last_attempt: i64 = sqlx::query_scalar(
        "SELECT last_delivery_attempt FROM edgeagent_message_quarantine WHERE consumer_name = $1 AND delivery_key = $2",
    )
    .bind("execution_simulator_v1")
    .bind(large_key)
    .fetch_one(&mut client)
    .await?;
    assert_eq!(last_attempt, 2);

    let replay_request = ReplayRequest::new(
        "inbound-replay-request-01",
        "urn:edgeagent:operator:alice",
        "consumer_remediated",
    )?;
    let mut transaction = client.begin().await?;
    let authorization = inbox
        .authorize_quarantine_replay(
            &mut transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:7",
            replay_request,
        )
        .await?;
    assert_eq!(authorization.disposition(), ReplayDisposition::Authorized);
    assert_eq!(
        authorization.transport_subject(),
        "edgeagent.command.execution.submit-dry-run-order.v1"
    );
    assert_eq!(authorization.payload(), poison_payload);
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    let repeated = inbox
        .authorize_quarantine_replay(
            &mut transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:7",
            replay_request,
        )
        .await?;
    assert_eq!(repeated.disposition(), ReplayDisposition::AlreadyAuthorized);
    assert_eq!(repeated.payload(), poison_payload);

    let conflict = inbox
        .authorize_quarantine_replay(
            &mut transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:7",
            ReplayRequest::new(
                "inbound-replay-request-01",
                "urn:edgeagent:operator:bob",
                "consumer_remediated",
            )?,
        )
        .await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(InboxErrorKind::ReplayRequestConflict)
    );

    let missing = inbox
        .authorize_quarantine_replay(
            &mut transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:404",
            ReplayRequest::new(
                "inbound-replay-request-02",
                "urn:edgeagent:operator:alice",
                "consumer_remediated",
            )?,
        )
        .await;
    assert_eq!(
        missing.err().map(|error| error.kind()),
        Some(InboxErrorKind::NotQuarantined)
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    let second_authorization = inbox
        .authorize_quarantine_replay(
            &mut transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:7",
            ReplayRequest::new(
                "inbound-replay-request-03",
                "urn:edgeagent:operator:alice",
                "consumer_remediated",
            )?,
        )
        .await?;
    assert_eq!(
        second_authorization.disposition(),
        ReplayDisposition::Authorized
    );
    transaction.commit().await?;

    let audit = sqlx::query(
        "SELECT requested_by, reason, prior_failure_code, prior_first_delivery_attempt, prior_last_delivery_attempt FROM edgeagent_message_quarantine_replay_audit WHERE replay_request_id = $1",
    )
        .bind("inbound-replay-request-01")
        .fetch_one(&mut client)
        .await?;
    assert_eq!(
        audit.try_get::<String, _>(0)?,
        "urn:edgeagent:operator:alice"
    );
    assert_eq!(audit.try_get::<String, _>(1)?, "consumer_remediated");
    assert_eq!(audit.try_get::<String, _>(2)?, "envelope_invalid");
    assert_eq!(audit.try_get::<i64, _>(3)?, 1);
    assert_eq!(audit.try_get::<i64, _>(4)?, i64::from(u32::MAX));
    let mut transaction = client.begin().await?;
    assert_eq!(
        inbox
            .authorize_quarantine_replay(
                &mut transaction,
                "execution_simulator_v1",
                maximum_attempt_key,
                ReplayRequest::new(
                    "inbound-replay-request-max-attempt",
                    "urn:edgeagent:operator:alice",
                    "consumer_remediated",
                )?,
            )
            .await?
            .disposition(),
        ReplayDisposition::Authorized
    );
    transaction.commit().await?;
    let maximum_audit = sqlx::query(
        "SELECT prior_first_delivery_attempt, prior_last_delivery_attempt FROM edgeagent_message_quarantine_replay_audit WHERE replay_request_id = $1",
    )
    .bind("inbound-replay-request-max-attempt")
    .fetch_one(&mut client)
    .await?;
    assert_eq!(maximum_audit.try_get::<i64, _>(0)?, i64::from(u32::MAX));
    assert_eq!(maximum_audit.try_get::<i64, _>(1)?, i64::from(u32::MAX));
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_quarantine_replay_audit")
            .fetch_one(&mut client)
            .await?;
    assert_eq!(audit_count, 3);

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn attempt_range_migration_preserves_existing_evidence() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_inbox_attempt_upgrade_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresInbox::MIGRATIONS.iter().take(3) {
        sqlx::raw_sql(*migration).execute(&mut client).await?;
    }

    sqlx::query(
        "INSERT INTO edgeagent_message_quarantine (consumer_name, delivery_key, transport_subject, payload, failure_code, first_delivery_attempt, last_delivery_attempt) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind("execution_simulator_v1")
    .bind("legacy:1")
    .bind("edgeagent.command.execution.submit-dry-run-order.v1")
    .bind(b"legacy-invalid".as_slice())
    .bind("envelope_invalid")
    .bind(3_i32)
    .bind(4_i32)
    .execute(&mut client)
    .await?;
    sqlx::query(
        "INSERT INTO edgeagent_message_quarantine_replay_audit (replay_request_id, consumer_name, delivery_key, requested_by, reason, prior_failure_code, prior_first_delivery_attempt, prior_last_delivery_attempt, prior_quarantined_at, prior_last_observed_at) SELECT 'legacy-replay', consumer_name, delivery_key, 'operator', 'remediated', failure_code, first_delivery_attempt, last_delivery_attempt, quarantined_at, last_observed_at FROM edgeagent_message_quarantine",
    )
    .execute(&mut client)
    .await?;

    sqlx::raw_sql(PostgresInbox::ATTEMPT_RANGE_MIGRATION_SQL)
        .execute(&mut client)
        .await?;
    let quarantine = sqlx::query(
        "SELECT first_delivery_attempt, last_delivery_attempt, pg_typeof(first_delivery_attempt)::text AS first_type, pg_typeof(last_delivery_attempt)::text AS last_type FROM edgeagent_message_quarantine WHERE delivery_key = 'legacy:1'",
    )
    .fetch_one(&mut client)
    .await?;
    assert_eq!(quarantine.try_get::<i64, _>(0)?, 3);
    assert_eq!(quarantine.try_get::<i64, _>(1)?, 4);
    assert_eq!(quarantine.try_get::<String, _>(2)?, "bigint");
    assert_eq!(quarantine.try_get::<String, _>(3)?, "bigint");
    let audit = sqlx::query(
        "SELECT prior_first_delivery_attempt, prior_last_delivery_attempt, pg_typeof(prior_first_delivery_attempt)::text AS first_type, pg_typeof(prior_last_delivery_attempt)::text AS last_type FROM edgeagent_message_quarantine_replay_audit WHERE replay_request_id = 'legacy-replay'",
    )
    .fetch_one(&mut client)
    .await?;
    assert_eq!(audit.try_get::<i64, _>(0)?, 3);
    assert_eq!(audit.try_get::<i64, _>(1)?, 4);
    assert_eq!(audit.try_get::<String, _>(2)?, "bigint");
    assert_eq!(audit.try_get::<String, _>(3)?, "bigint");

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}
