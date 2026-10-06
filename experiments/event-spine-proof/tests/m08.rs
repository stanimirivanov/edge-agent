//! Negative control: current message-level recovery is not step-level workflow recovery.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config as StreamConfig, RetentionPolicy, StorageType};
use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRegistry};
use edgeagent_inbox_postgres::PostgresInbox;
use edgeagent_messaging::MessagePublisher;
use edgeagent_messaging_nats::JetStreamPublisher;
use serde_json::json;
use sqlx::{Connection, PgConnection, Row};
use std::{error::Error, io, process::Stdio, time::Duration};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::process::Command;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const POSTGRES_IMAGE: &str =
    "18.6-alpine3.23@sha256:885cf05d376c7cf27afef02073e6bdac3841252537f16e244fd1c1e6a7c99fb1";
const NATS_IMAGE: &str =
    "2.14.7-alpine@sha256:4063edae0717ba5f7501bfde75f97fd9b57f5b93597b92c70b6a6fbbf6a74e06";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn m08_event_spine_alone_does_not_resume_post_ack_step_b() -> TestResult {
    let postgres = GenericImage::new("postgres", POSTGRES_IMAGE)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "m08")
        .with_env_var("POSTGRES_USER", "m08")
        .with_env_var("POSTGRES_PASSWORD", "m08")
        .start()
        .await?;
    let nats = GenericImage::new("nats", NATS_IMAGE)
        .with_exposed_port(4222.tcp())
        .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
        .with_cmd(["--jetstream"])
        .start()
        .await?;

    let postgres_url = format!(
        "postgresql://m08:m08@{}:{}/m08",
        postgres.get_host().await?,
        postgres.get_host_port_ipv4(5432).await?
    );
    let nats_url = format!(
        "nats://{}:{}",
        nats.get_host().await?,
        nats.get_host_port_ipv4(4222).await?
    );
    let mut database = PgConnection::connect(&postgres_url).await?;
    for migration in PostgresInbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut database).await?;
    }
    sqlx::raw_sql(
        "CREATE TABLE m08_step_a (id INTEGER PRIMARY KEY);
             CREATE TABLE m08_step_b_started (id INTEGER PRIMARY KEY);
             CREATE TABLE m08_step_b_completed (id INTEGER PRIMARY KEY);",
    )
    .execute(&mut database)
    .await?;

    let nats_client = async_nats::connect(&nats_url).await?;
    let context = jetstream::new(nats_client);
    let stream = context
        .create_stream(StreamConfig {
            name: "M08_SPINE".to_owned(),
            subjects: vec![COMMAND.subject()?],
            storage: StorageType::File,
            retention: RetentionPolicy::WorkQueue,
            ..StreamConfig::default()
        })
        .await?;
    stream
        .get_or_create_consumer(
            "m08_worker",
            pull::Config {
                durable_name: Some("m08_worker".to_owned()),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_millis(750),
                max_deliver: 5,
                max_ack_pending: 1,
                max_batch: 1,
                filter_subject: COMMAND.subject()?,
                ..pull::Config::default()
            },
        )
        .await?;
    let command = COMMAND.build(
        MessageMetadata {
            id: "m08-event-spine-01".to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: "order/m08-event-spine-01".to_owned(),
            time: "2026-10-05T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: "m08-correlation-01".to_owned(),
            causation_id: "m08-request-01".to_owned(),
            idempotency_key: "m08-event-spine-01".to_owned(),
            partition_key: "order/m08-event-spine-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run", "quantity": 1, "as_of": "2026-10-05T00:00:00Z"}),
    )?;
    let definitions = [COMMAND];
    MessageRegistry::new(&definitions)?.validate(&command)?;
    let publisher = JetStreamPublisher::from_context(context.clone());
    publisher.publish(COMMAND, &command).await?;

    let mut first = Command::new(env!("CARGO_BIN_EXE_m08_event_spine_worker"))
        .arg(&postgres_url)
        .arg(&nats_url)
        .arg("hang-at-b")
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM m08_step_b_started")
                .fetch_one(&mut database)
                .await?;
            if count == 1 {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await??;
    first.kill().await?;
    assert!(
        !first.wait().await?.success(),
        "worker must die inside Step B"
    );

    let restarted = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(env!("CARGO_BIN_EXE_m08_event_spine_worker"))
            .arg(&postgres_url)
            .arg(&nats_url)
            .arg("complete")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    if !restarted.status.success() {
        return Err(io::Error::other(format!(
            "restarted worker failed: {}",
            String::from_utf8_lossy(&restarted.stderr)
        ))
        .into());
    }
    assert!(
        String::from_utf8_lossy(&restarted.stdout).contains("NO_PENDING_DELIVERY"),
        "a completed message was acknowledged before Step B"
    );

    let counts = sqlx::query(
        "SELECT
                (SELECT count(*) FROM m08_step_a),
                (SELECT count(*) FROM m08_step_b_started),
                (SELECT count(*) FROM m08_step_b_completed),
                (SELECT count(*) FROM edgeagent_message_inbox)",
    )
    .fetch_one(&mut database)
    .await?;
    assert_eq!(counts.try_get::<i64, _>(0)?, 1, "Step A did not repeat");
    assert_eq!(counts.try_get::<i64, _>(1)?, 1, "Step B started once");
    assert_eq!(counts.try_get::<i64, _>(2)?, 0, "Step B was not recovered");
    assert_eq!(
        counts.try_get::<i64, _>(3)?,
        1,
        "inbox transition committed once"
    );
    Ok(())
}
