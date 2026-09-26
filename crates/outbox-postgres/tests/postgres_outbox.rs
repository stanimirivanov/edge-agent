//! Opt-in conformance test against the checked-in local PostgreSQL profile.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageMetadata, MessageRegistry, RetentionClass,
};
use edgeagent_outbox_postgres::{EnqueueDisposition, OutboxErrorKind, PostgresOutbox};
use serde_json::json;
use std::env;
use std::error::Error;
use std::process;
use std::time::Duration;
use tokio_postgres::NoTls;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const EVENT: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.execution.dry-run-order-accepted.v1",
    "urn:edgeagent:schema:dry-run-order-accepted:v1",
    Component::ExecutionSimulator,
    "order",
    RetentionClass::AuditEvent,
);

fn metadata(source: Component) -> MessageMetadata {
    MessageMetadata {
        id: "outbox-message-01".to_owned(),
        source: source.source_uri().to_owned(),
        message_type: COMMAND.message_type.to_owned(),
        subject: "order/outbox-order-01".to_owned(),
        time: "2026-09-26T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema.to_owned(),
        correlation_id: "outbox-correlation-01".to_owned(),
        causation_id: "outbox-request-01".to_owned(),
        idempotency_key: "outbox-order-01".to_owned(),
        partition_key: "order/outbox-order-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn transaction_identity_and_lease_invariants_hold() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_outbox_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    client.batch_execute(PostgresOutbox::MIGRATION_SQL).await?;

    let outbox = PostgresOutbox;
    let gateway_envelope = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 1}))?;
    let research_envelope =
        COMMAND.build(metadata(Component::Research), &json!({"quantity": 1}))?;

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.rollback().await?;
    let row = client
        .query_one("SELECT count(*) FROM edgeagent_message_outbox", &[])
        .await?;
    assert_eq!(row.try_get::<_, i64>(0)?, 0);

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::AlreadyPresent
    );
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &research_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;

    let conflicting = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 2}))?;
    let transaction = client.transaction().await?;
    let conflict = outbox.enqueue(&transaction, COMMAND, &conflicting).await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(OutboxErrorKind::MessageIdentityConflict)
    );
    transaction.rollback().await?;

    let transaction = client.transaction().await?;
    let claimed = outbox
        .claim_batch(&transaction, "relay_01", 10, Duration::from_secs(30))
        .await?;
    assert_eq!(claimed.len(), 2);
    let definitions = [COMMAND, EVENT];
    let registry = MessageRegistry::new(&definitions)?;
    for message in &claimed {
        assert_eq!(message.attempt(), 1);
        assert_eq!(
            message.validated_envelope(&registry)?.to_json()?,
            message.envelope_bytes()
        );
    }
    transaction.commit().await?;

    let first = &claimed[0];
    let second = &claimed[1];
    let transaction = client.transaction().await?;
    let lost = outbox
        .mark_published(
            &transaction,
            first.message_source(),
            first.message_id(),
            "relay_02",
        )
        .await;
    assert_eq!(
        lost.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    transaction.rollback().await?;

    let transaction = client.transaction().await?;
    outbox
        .release_for_retry(
            &transaction,
            first.message_source(),
            first.message_id(),
            "relay_01",
            Duration::ZERO,
            "transport_unavailable",
        )
        .await?;
    outbox
        .mark_published(
            &transaction,
            second.message_source(),
            second.message_id(),
            "relay_01",
        )
        .await?;
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    let retried = outbox
        .claim_batch(&transaction, "relay_02", 10, Duration::from_secs(30))
        .await?;
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].attempt(), 2);
    outbox
        .mark_published(
            &transaction,
            retried[0].message_source(),
            retried[0].message_id(),
            "relay_02",
        )
        .await?;
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert!(
        outbox
            .claim_batch(&transaction, "relay_03", 10, Duration::from_secs(30))
            .await?
            .is_empty()
    );
    transaction.commit().await?;

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}
