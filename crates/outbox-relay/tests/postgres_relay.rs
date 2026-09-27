//! Opt-in relay conformance test against the checked-in local PostgreSQL profile.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_messaging::{
    MessagePublisher, PublishDisposition, PublishError, PublishErrorKind, PublishFuture,
    PublishReceipt,
};
use edgeagent_outbox_postgres::PostgresOutbox;
use edgeagent_outbox_relay::{QuarantineReason, RelayOutcome, RelayPolicy, relay_once};
use serde_json::json;
use std::collections::VecDeque;
use std::env;
use std::error::Error;
use std::io;
use std::process;
use std::sync::Mutex;
use std::time::Duration;
use tokio_postgres::NoTls;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn envelope(message_id: &str) -> Result<MessageEnvelope, Box<dyn Error>> {
    let metadata = MessageMetadata {
        id: message_id.to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: COMMAND.message_type.to_owned(),
        subject: format!("order/{message_id}"),
        time: "2026-09-27T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema.to_owned(),
        correlation_id: "relay-correlation-01".to_owned(),
        causation_id: "relay-request-01".to_owned(),
        idempotency_key: message_id.to_owned(),
        partition_key: format!("order/{message_id}"),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    };
    Ok(COMMAND.build(metadata, &json!({"quantity": 1}))?)
}

enum StubOutcome {
    Persisted,
    Failure(PublishErrorKind),
}

struct StubPublisher {
    outcomes: Mutex<VecDeque<StubOutcome>>,
}

impl StubPublisher {
    fn new(outcomes: impl IntoIterator<Item = StubOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
        }
    }
}

impl MessagePublisher for StubPublisher {
    fn publish<'publisher>(
        &'publisher self,
        _definition: MessageDefinition,
        _envelope: &'publisher MessageEnvelope,
    ) -> PublishFuture<'publisher> {
        Box::pin(async move {
            let outcome = self
                .outcomes
                .lock()
                .map_err(|_| {
                    PublishError::with_source(
                        PublishErrorKind::Unavailable,
                        io::Error::other("stub publisher lock poisoned"),
                    )
                })?
                .pop_front()
                .ok_or_else(|| {
                    PublishError::with_source(
                        PublishErrorKind::Unavailable,
                        io::Error::other("stub publisher has no configured outcome"),
                    )
                })?;
            match outcome {
                StubOutcome::Persisted => Ok(PublishReceipt::new(PublishDisposition::Persisted)),
                StubOutcome::Failure(kind) => Err(PublishError::with_source(
                    kind,
                    io::Error::other("synthetic publication failure"),
                )),
            }
        })
    }
}

async fn enqueue(
    client: &mut tokio_postgres::Client,
    message_id: &str,
) -> Result<(), Box<dyn Error>> {
    let transaction = client.transaction().await?;
    PostgresOutbox
        .enqueue(&transaction, COMMAND, &envelope(message_id)?)
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn relay_bounds_retry_and_quarantines_terminal_failures() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_relay_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    client.batch_execute(PostgresOutbox::MIGRATION_SQL).await?;
    enqueue(&mut client, "relay-success-01").await?;
    client
        .batch_execute(PostgresOutbox::QUARANTINE_MIGRATION_SQL)
        .await?;

    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "relay_test",
        Duration::from_secs(30),
        2,
        Duration::from_secs(2),
        Duration::from_secs(30),
    )?;
    let publisher = StubPublisher::new([
        StubOutcome::Persisted,
        StubOutcome::Failure(PublishErrorKind::Unavailable),
        StubOutcome::Failure(PublishErrorKind::ConfirmationUnknown),
        StubOutcome::Failure(PublishErrorKind::Rejected),
    ]);

    assert!(matches!(
        relay_once(&mut client, &registry, &publisher, &policy).await?,
        RelayOutcome::Published {
            disposition: PublishDisposition::Persisted,
            attempt: 1,
        }
    ));

    enqueue(&mut client, "relay-exhaust-01").await?;
    let first_failure = relay_once(&mut client, &registry, &publisher, &policy).await?;
    assert!(matches!(
        first_failure,
        RelayOutcome::RetryScheduled {
            failure,
            attempt: 1,
            ..
        } if failure.kind() == PublishErrorKind::Unavailable
    ));
    client
        .execute(
            "UPDATE edgeagent_message_outbox SET available_at = clock_timestamp() WHERE message_id = $1",
            &[&"relay-exhaust-01"],
        )
        .await?;
    assert!(matches!(
        relay_once(&mut client, &registry, &publisher, &policy).await?,
        RelayOutcome::Quarantined {
            reason: QuarantineReason::AttemptsExhaustedConfirmationUnknown,
            attempt: 2,
            failure: _,
        }
    ));

    enqueue(&mut client, "relay-rejected-01").await?;
    assert!(matches!(
        relay_once(&mut client, &registry, &publisher, &policy).await?,
        RelayOutcome::Quarantined {
            reason: QuarantineReason::TransportRejected,
            attempt: 1,
            failure: _,
        }
    ));
    assert!(matches!(
        relay_once(&mut client, &registry, &publisher, &policy).await?,
        RelayOutcome::Idle
    ));

    let row = client
        .query_one(
            "SELECT count(*) FILTER (WHERE published_at IS NOT NULL), count(*) FILTER (WHERE quarantined_at IS NOT NULL), count(*) FILTER (WHERE quarantine_reason = 'attempts_exhausted_confirmation_unknown'), count(*) FILTER (WHERE quarantine_reason = 'transport_rejected') FROM edgeagent_message_outbox",
            &[],
        )
        .await?;
    assert_eq!(row.try_get::<_, i64>(0)?, 1);
    assert_eq!(row.try_get::<_, i64>(1)?, 2);
    assert_eq!(row.try_get::<_, i64>(2)?, 1);
    assert_eq!(row.try_get::<_, i64>(3)?, 1);

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}
