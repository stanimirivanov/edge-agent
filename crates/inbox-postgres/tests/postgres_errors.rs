//! Opt-in PostgreSQL error-classification regressions against an isolated schema.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_postgres::{
    InboxErrorKind, PostgresHandlerFuture, PostgresInboundMessageStore, PostgresInbox,
    PostgresTransactionalMessageHandler, QuarantineDisposition, QuarantineEvidence, ReplayRequest,
};
use edgeagent_messaging::{
    InboundMessageStore, InboundProcessingError, InboxDisposition, InboxStoreErrorKind,
};
use serde_json::json;
use std::env;
use std::error::Error;
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_postgres::{NoTls, Transaction};

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
        subject: "order/inbox-error-01".to_owned(),
        time: "2026-09-27T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema().to_owned(),
        correlation_id: "inbox-error-correlation-01".to_owned(),
        causation_id: "inbox-error-request-01".to_owned(),
        idempotency_key: "inbox-error-01".to_owned(),
        partition_key: "order/inbox-error-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

struct CountingHandler(AtomicUsize);

impl PostgresTransactionalMessageHandler for CountingHandler {
    fn handle<'handler>(
        &'handler self,
        _transaction: &'handler Transaction<'_>,
        _envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

fn assert_invariant_with_source(
    result: Result<InboxDisposition, InboundProcessingError>,
) -> Result<(), Box<dyn Error>> {
    let Err(InboundProcessingError::Store(error)) = result else {
        return Err("expected an inbound storage failure".into());
    };
    assert_eq!(error.kind(), InboxStoreErrorKind::Invariant);
    assert_eq!(error.to_string(), "inbox storage invariant failed");
    assert!(error.source().and_then(Error::source).is_some());
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn permanent_schema_faults_are_invariants_through_inbound_port() -> Result<(), Box<dyn Error>>
{
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_inbox_fault_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;

    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let envelope = COMMAND.build(metadata("inbox-fault-01"), &json!({"quantity": 1}))?;
    let handler = CountingHandler(AtomicUsize::new(0));

    let missing_table = PostgresInboundMessageStore::new(&mut client, &handler)
        .process("execution_simulator_v1", &registry, &envelope)
        .await;
    assert_invariant_with_source(missing_table)?;
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);

    for migration in PostgresInbox::MIGRATIONS {
        client.batch_execute(migration).await?;
    }
    assert_eq!(
        PostgresInboundMessageStore::new(&mut client, &handler)
            .process("execution_simulator_v1", &registry, &envelope)
            .await?,
        InboxDisposition::Applied
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);

    // A persisted-column schema drift must not enter the availability retry path.
    client
        .batch_execute(
            "ALTER TABLE edgeagent_message_inbox ALTER COLUMN message_type TYPE INTEGER USING 1",
        )
        .await?;
    let wrong_column_type = PostgresInboundMessageStore::new(&mut client, &handler)
        .process("execution_simulator_v1", &registry, &envelope)
        .await;
    assert_invariant_with_source(wrong_column_type)?;
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn closed_postgres_connection_remains_unavailable_through_inbound_port()
-> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    connection_task.abort();
    let _ = connection_task.await;

    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let envelope = COMMAND.build(metadata("inbox-unavailable-01"), &json!({"quantity": 1}))?;
    let handler = CountingHandler(AtomicUsize::new(0));
    let result = PostgresInboundMessageStore::new(&mut client, &handler)
        .process("execution_simulator_v1", &registry, &envelope)
        .await;
    assert!(matches!(
        result,
        Err(InboundProcessingError::Store(error))
            if error.kind() == InboxStoreErrorKind::Unavailable
    ));
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn replay_row_decode_fault_is_invariant_and_audit_rolls_back() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_inbox_decode_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in PostgresInbox::MIGRATIONS {
        client.batch_execute(migration).await?;
    }

    let transaction = client.transaction().await?;
    assert_eq!(
        PostgresInbox
            .quarantine_delivery(
                &transaction,
                "execution_simulator_v1",
                QuarantineEvidence::new(
                    "18:EDGEAGENT_COMMANDS:decode",
                    "edgeagent.command.execution.submit-dry-run-order.v1",
                    1,
                    b"{synthetic-invalid-payload}",
                    "envelope_invalid",
                )?,
            )
            .await?,
        QuarantineDisposition::Inserted
    );
    transaction.commit().await?;

    // The replay CTE can still insert its audit row, but its selected TEXT
    // payload cannot be decoded as the BYTEA that this adapter requires.
    client
        .batch_execute(
            "ALTER TABLE edgeagent_message_quarantine ALTER COLUMN payload TYPE TEXT USING encode(payload, 'hex')",
        )
        .await?;
    let transaction = client.transaction().await?;
    let replay = PostgresInbox
        .authorize_quarantine_replay(
            &transaction,
            "execution_simulator_v1",
            "18:EDGEAGENT_COMMANDS:decode",
            ReplayRequest::new(
                "decode-replay-request-01",
                "urn:edgeagent:operator:alice",
                "consumer_remediated",
            )?,
        )
        .await;
    let Err(error) = replay else {
        return Err("expected a replay row-decode failure".into());
    };
    assert_eq!(error.kind(), InboxErrorKind::StorageInvariant);
    assert_eq!(error.to_string(), "inbox storage invariant failed");
    assert!(error.source().is_some());
    let uncommitted_audit = transaction
        .query_one(
            "SELECT count(*) FROM edgeagent_message_quarantine_replay_audit",
            &[],
        )
        .await?;
    assert_eq!(uncommitted_audit.try_get::<_, i64>(0)?, 1);
    transaction.rollback().await?;
    let committed_audit = client
        .query_one(
            "SELECT count(*) FROM edgeagent_message_quarantine_replay_audit",
            &[],
        )
        .await?;
    assert_eq!(committed_audit.try_get::<_, i64>(0)?, 0);

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}
