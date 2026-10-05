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
    assert_eq!(quarantine.try_get::<Option<i32>, _>(1)?, Some(2));

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
    transaction.rollback().await?;

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
    assert_eq!(audit.try_get::<i32, _>(3)?, 1);
    assert_eq!(audit.try_get::<i32, _>(4)?, 2);
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_quarantine_replay_audit")
            .fetch_one(&mut client)
            .await?;
    assert_eq!(audit_count, 2);

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}
