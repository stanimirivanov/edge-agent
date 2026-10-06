//! Opt-in proof that inbox, service state, and outbound enqueue share one transaction.

use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_postgres::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresInbox,
    PostgresTransactionalMessageHandler,
};
use edgeagent_messaging::{InboundMessageStore, InboundProcessingError, InboxDisposition};
use edgeagent_outbox_postgres::{EnqueueDisposition, OutboxErrorKind, PostgresOutbox};
use serde_json::json;
use sqlx::{Connection, PgConnection, Postgres, Row, Transaction};
use std::{
    env,
    error::Error,
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

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
    EventRetention::Audit,
);

fn envelope(
    definition: MessageDefinition,
    source: Component,
    payload: &str,
) -> Result<MessageEnvelope, Box<dyn Error>> {
    let id = if definition == COMMAND {
        "atomic-command-01"
    } else {
        "atomic-event-01"
    };
    Ok(definition.build(
        MessageMetadata {
            id: id.to_owned(),
            source: source.source_uri().to_owned(),
            message_type: definition.message_type().to_owned(),
            subject: "order/atomic-01".to_owned(),
            time: "2026-10-06T00:00:00Z".to_owned(),
            data_schema: definition.data_schema().to_owned(),
            correlation_id: "atomic-correlation-01".to_owned(),
            causation_id: "atomic-command-01".to_owned(),
            idempotency_key: id.to_owned(),
            partition_key: "order/atomic-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run", "state": payload}),
    )?)
}

struct EmittingHandler {
    event: MessageEnvelope,
    calls: AtomicUsize,
}

impl PostgresTransactionalMessageHandler for EmittingHandler {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        _envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            sqlx::query("INSERT INTO domain_effects (id) VALUES (1)")
                .execute(&mut **transaction)
                .await
                .map_err(|error| {
                    edgeagent_inbox_handler::HandlerFailure::with_source(
                        edgeagent_inbox_handler::HandlerFailureKind::Transient,
                        "storage_unavailable",
                        error,
                    )
                })?;
            PostgresOutbox
                .enqueue(transaction, EVENT, &self.event)
                .await
                .map_err(|error| {
                    edgeagent_inbox_handler::HandlerFailure::with_source(
                        edgeagent_inbox_handler::HandlerFailureKind::Transient,
                        "outbox_unavailable",
                        error,
                    )
                })?;
            if call == 1 {
                return Err(edgeagent_inbox_handler::HandlerFailure::transient(
                    "injected_failure",
                ));
            }
            Ok(())
        })
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn inbox_domain_and_sqlx_outbox_commit_or_roll_back_together() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_atomic_outbox_test_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresInbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }
    for migration in PostgresOutbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }
    sqlx::raw_sql("CREATE TABLE domain_effects (id INTEGER PRIMARY KEY)")
        .execute(&mut client)
        .await?;

    let command = envelope(COMMAND, Component::Gateway, "submitted")?;
    let event = envelope(EVENT, Component::ExecutionSimulator, "accepted")?;
    let handler = EmittingHandler {
        event: event.clone(),
        calls: AtomicUsize::new(0),
    };
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;

    {
        let mut store = PostgresInboundMessageStore::new(&mut client, &handler);
        assert!(matches!(
            store
                .process("execution_simulator_v1", &registry, &command)
                .await,
            Err(InboundProcessingError::Handler(_))
        ));
    }
    let rolled_back = sqlx::query(
        "SELECT (SELECT count(*) FROM edgeagent_message_inbox), (SELECT count(*) FROM domain_effects), (SELECT count(*) FROM edgeagent_message_outbox)",
    )
    .fetch_one(&mut client)
    .await?;
    for column in 0..3 {
        assert_eq!(rolled_back.try_get::<i64, _>(column)?, 0);
    }

    {
        let mut store = PostgresInboundMessageStore::new(&mut client, &handler);
        assert_eq!(
            store
                .process("execution_simulator_v1", &registry, &command)
                .await?,
            InboxDisposition::Applied
        );
        assert_eq!(
            store
                .process("execution_simulator_v1", &registry, &command)
                .await?,
            InboxDisposition::Duplicate
        );
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
    let committed = sqlx::query(
        "SELECT (SELECT count(*) FROM edgeagent_message_inbox), (SELECT count(*) FROM domain_effects), (SELECT count(*) FROM edgeagent_message_outbox)",
    )
    .fetch_one(&mut client)
    .await?;
    for column in 0..3 {
        assert_eq!(committed.try_get::<i64, _>(column)?, 1);
    }

    let mut transaction = client.begin().await?;
    assert_eq!(
        PostgresOutbox
            .enqueue(&mut transaction, EVENT, &event)
            .await?,
        EnqueueDisposition::AlreadyPresent
    );
    let changed = envelope(EVENT, Component::ExecutionSimulator, "changed")?;
    assert_eq!(
        PostgresOutbox
            .enqueue(&mut transaction, EVENT, &changed)
            .await
            .err()
            .map(|error| error.kind()),
        Some(OutboxErrorKind::MessageIdentityConflict)
    );
    let stored_bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT envelope FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
    )
    .bind(event.source())
    .bind(event.id())
    .fetch_one(&mut *transaction)
    .await?;
    assert_eq!(stored_bytes, event.to_json()?);
    transaction.rollback().await?;

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}
