//! Opt-in end-to-end event-spine recovery conformance.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, PullConsumer, pull};
use async_nats::jetstream::stream::{Config as StreamConfig, RetentionPolicy, StorageType};
use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_handler::{HandlerFailure, HandlerPolicy, HandlingOutcome, handle_once};
use edgeagent_inbox_postgres::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresInbox,
    PostgresTransactionalMessageHandler,
};
use edgeagent_messaging::{MessageConsumer, PublishDisposition};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use edgeagent_outbox_postgres::{PostgresOutbox, PostgresOutboxRelay};
use edgeagent_outbox_relay::{RelayOutcome, RelayPolicy, relay_once};
use serde_json::json;
use std::env;
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::{self, Command};
use std::time::{Duration, Instant};
use tokio_postgres::{NoTls, Transaction};

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

const BROKER_RESTART_OPT_IN: &str = "EDGEAGENT_NATS_COMPOSE_RESTART";

#[derive(Clone, Copy)]
enum RecoveryMode {
    Client,
    Broker,
}

impl RecoveryMode {
    const fn name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Broker => "broker",
        }
    }

    const fn storage(self) -> StorageType {
        match self {
            Self::Client => StorageType::Memory,
            Self::Broker => StorageType::File,
        }
    }
}

fn envelope(message_id: &str) -> Result<MessageEnvelope, Box<dyn Error + Send + Sync>> {
    Ok(COMMAND.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: format!("order/{message_id}"),
            time: "2026-09-28T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: "event-spine-correlation-01".to_owned(),
            causation_id: "event-spine-request-01".to_owned(),
            idempotency_key: message_id.to_owned(),
            partition_key: format!("order/{message_id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run", "quantity": 1}),
    )?)
}

struct EffectHandler;

impl PostgresTransactionalMessageHandler for EffectHandler {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler Transaction<'_>,
        envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move {
            transaction
                .execute(
                    "INSERT INTO event_spine_effects (message_source, message_id) VALUES ($1, $2)",
                    &[&envelope.source(), &envelope.id()],
                )
                .await
                .map_err(|error| {
                    HandlerFailure::with_source(
                        edgeagent_inbox_handler::HandlerFailureKind::Transient,
                        "storage_unavailable",
                        error,
                    )
                })?;
            Ok(())
        })
    }
}

fn consumer_config(name: &str, subject: String) -> pull::Config {
    pull::Config {
        durable_name: Some(name.to_owned()),
        ack_policy: AckPolicy::Explicit,
        ack_wait: Duration::from_millis(500),
        max_deliver: 5,
        max_ack_pending: 1,
        max_batch: 1,
        filter_subject: subject,
        ..pull::Config::default()
    }
}

async fn reconnect_nats(
    nats_url: &str,
) -> Result<async_nats::Client, Box<dyn Error + Send + Sync>> {
    let deadline = Instant::now() + Duration::from_secs(30);

    loop {
        match tokio::time::timeout(Duration::from_secs(2), async_nats::connect(nats_url)).await {
            Ok(Ok(client)) => return Ok(client),
            Ok(Err(_)) | Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(error.into()),
        }
    }
}

async fn restart_nats_with_compose() -> Result<(), Box<dyn Error + Send + Sync>> {
    if env::var(BROKER_RESTART_OPT_IN).as_deref() != Ok("1") {
        return Err(io::Error::other(format!(
            "set {BROKER_RESTART_OPT_IN}=1 to authorize the isolated Compose broker restart"
        ))
        .into());
    }

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let command_root = repository_root.clone();
    let status = tokio::task::spawn_blocking(move || {
        Command::new("docker")
            .arg("compose")
            .arg("--env-file")
            .arg(command_root.join("deploy/local/.env.example"))
            .arg("-f")
            .arg(command_root.join("deploy/local/compose.yaml"))
            .arg("restart")
            .arg("nats")
            .current_dir(command_root)
            .status()
    })
    .await??;

    if !status.success() {
        return Err(io::Error::other(format!(
            "Compose failed to restart NATS with status {status}"
        ))
        .into());
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL, EDGEAGENT_POSTGRES_URL, and isolated services"]
async fn durable_command_survives_client_restart_with_one_domain_transition()
-> Result<(), Box<dyn Error + Send + Sync>> {
    verify_restart_recovery(RecoveryMode::Client).await
}

#[tokio::test]
#[ignore = "requires isolated Compose services and EDGEAGENT_NATS_COMPOSE_RESTART=1"]
async fn durable_command_survives_broker_restart_with_one_domain_transition()
-> Result<(), Box<dyn Error + Send + Sync>> {
    verify_restart_recovery(RecoveryMode::Broker).await
}

async fn verify_restart_recovery(
    recovery_mode: RecoveryMode,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let nats_url = env::var("EDGEAGENT_NATS_URL")?;
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let process_id = process::id();
    let mode_name = recovery_mode.name();
    let stream_name = format!("EDGEAGENT_SPINE_{}_{process_id}", mode_name.to_uppercase());
    let consumer_name = format!("event_spine_{mode_name}_v1_{process_id}");
    let schema = format!("edgeagent_spine_{mode_name}_test_{process_id}");

    let (mut database, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    database
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in PostgresOutbox::MIGRATIONS {
        database.batch_execute(migration).await?;
    }
    for migration in PostgresInbox::MIGRATIONS {
        database.batch_execute(migration).await?;
    }
    database
        .batch_execute(
            "CREATE TABLE event_spine_effects (message_source TEXT NOT NULL, message_id VARCHAR(128) NOT NULL, PRIMARY KEY (message_source, message_id))",
        )
        .await?;

    let command = envelope(&format!("event-spine-{mode_name}-restart-01"))?;
    let transaction = database.transaction().await?;
    PostgresOutbox
        .enqueue(&transaction, COMMAND, &command)
        .await?;
    transaction.commit().await?;

    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let relay_policy = RelayPolicy::new(
        format!("event_spine_relay_{process_id}"),
        Duration::from_secs(10),
        3,
        Duration::from_millis(10),
        Duration::from_secs(1),
    )?;

    let first_client = async_nats::connect(&nats_url).await?;
    let first_context = jetstream::new(first_client);
    let first_stream = first_context
        .create_stream(StreamConfig {
            name: stream_name.clone(),
            subjects: vec![COMMAND.subject()?],
            storage: recovery_mode.storage(),
            retention: RetentionPolicy::WorkQueue,
            ..StreamConfig::default()
        })
        .await?;
    let first_consumer = first_stream
        .get_or_create_consumer(
            &consumer_name,
            consumer_config(&consumer_name, COMMAND.subject()?),
        )
        .await?;
    let publisher = JetStreamPublisher::from_context(first_context.clone());
    let relay_outcome = {
        let mut relay_store = PostgresOutboxRelay::new(&mut database);
        relay_once(&mut relay_store, &registry, &publisher, &relay_policy).await?
    };
    assert!(matches!(
        relay_outcome,
        RelayOutcome::Published {
            disposition: PublishDisposition::Persisted,
            attempt: 1,
        }
    ));

    let mut first_adapter = JetStreamConsumer::new(first_consumer).await?;
    let abandoned = tokio::time::timeout(Duration::from_secs(5), first_adapter.receive()).await??;
    assert_eq!(abandoned.metadata().delivery_attempt(), 1);
    let delivery_key = abandoned.metadata().message_key().to_owned();
    drop(abandoned);
    drop(first_adapter);
    drop(first_stream);
    drop(publisher);
    drop(first_context);

    match recovery_mode {
        RecoveryMode::Client => tokio::time::sleep(Duration::from_millis(750)).await,
        RecoveryMode::Broker => restart_nats_with_compose().await?,
    }

    let restarted_client = reconnect_nats(&nats_url).await?;
    let restarted_context = jetstream::new(restarted_client);
    let restarted_stream = restarted_context.get_stream(&stream_name).await?;
    let restarted_consumer: PullConsumer = restarted_stream.get_consumer(&consumer_name).await?;
    let mut inspector = restarted_consumer.clone();
    let mut restarted_adapter = JetStreamConsumer::new(restarted_consumer).await?;
    let redelivery =
        tokio::time::timeout(Duration::from_secs(5), restarted_adapter.receive()).await??;
    let redelivery_attempt = redelivery.metadata().delivery_attempt();
    assert!(redelivery_attempt >= 2);
    assert_eq!(redelivery.metadata().message_key(), delivery_key);
    assert_eq!(redelivery.payload(), command.to_json()?);

    let handler_policy = HandlerPolicy::new(
        &consumer_name,
        5,
        Duration::from_millis(10),
        Duration::from_secs(1),
    )?;
    let outcome = {
        let mut store = PostgresInboundMessageStore::new(&mut database, &EffectHandler);
        handle_once(&mut store, &registry, &handler_policy, redelivery).await?
    };
    assert!(matches!(
        outcome,
        HandlingOutcome::Applied { attempt } if attempt == redelivery_attempt
    ));

    let counts = database
        .query_one(
            "SELECT (SELECT count(*) FROM event_spine_effects), (SELECT count(*) FROM edgeagent_message_inbox), (SELECT count(*) FROM edgeagent_message_outbox WHERE published_at IS NOT NULL)",
            &[],
        )
        .await?;
    assert_eq!(counts.try_get::<_, i64>(0)?, 1);
    assert_eq!(counts.try_get::<_, i64>(1)?, 1);
    assert_eq!(counts.try_get::<_, i64>(2)?, 1);

    let consumer_info = inspector.info().await?;
    assert_eq!(consumer_info.num_ack_pending, 0);
    assert_eq!(consumer_info.num_pending, 0);

    assert!(restarted_context.delete_stream(&stream_name).await?.success);
    database
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}
