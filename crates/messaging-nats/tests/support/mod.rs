//! Shared fixtures for isolated, synthetic JetStream conformance executables.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{Component, MessageDefinition, MessageEnvelope, MessageMetadata};
use futures_util::FutureExt;
use serde_json::json;
use std::error::Error;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub type TestError = Box<dyn Error + Send + Sync>;
pub type TestResult = Result<(), TestError>;
pub const DEADLINE: Duration = Duration::from_secs(10);
pub const CASE_DEADLINE: Duration = Duration::from_secs(60);
static NEXT_RESOURCE: AtomicU64 = AtomicU64::new(1);

pub fn resource(prefix: &str) -> String {
    format!(
        "{prefix}_{}_{}",
        std::process::id(),
        NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed)
    )
}

pub async fn connect() -> Result<jetstream::Context, TestError> {
    let client = tokio::time::timeout(
        DEADLINE,
        async_nats::connect(std::env::var("EDGEAGENT_NATS_URL")?),
    )
    .await??;
    Ok(jetstream::ContextBuilder::new()
        .timeout(DEADLINE)
        .build(client))
}

pub fn stream_config(subjects: &[&str]) -> Config {
    Config {
        name: resource("EDGEAGENT_CONFORMANCE"),
        subjects: subjects
            .iter()
            .map(|subject| (*subject).to_owned())
            .collect(),
        storage: StorageType::Memory,
        ..Config::default()
    }
}

pub fn consumer_config(name: &str, subject: &str) -> pull::Config {
    pull::Config {
        durable_name: Some(name.to_owned()),
        ack_policy: AckPolicy::Explicit,
        filter_subject: subject.to_owned(),
        ack_wait: Duration::from_secs(120),
        ..pull::Config::default()
    }
}

pub async fn durable(
    stream: &jetstream::stream::Stream,
    name: &str,
    subject: &str,
) -> Result<jetstream::consumer::PullConsumer, TestError> {
    Ok(stream
        .create_consumer(consumer_config(name, subject))
        .await?)
}

pub fn metadata(definition: MessageDefinition, id: &str) -> MessageMetadata {
    MessageMetadata {
        id: id.to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: definition.message_type().to_owned(),
        subject: format!("order/{id}"),
        time: "2026-10-13T00:00:00Z".to_owned(),
        data_schema: definition.data_schema().to_owned(),
        correlation_id: "conformance-correlation-01".to_owned(),
        causation_id: "conformance-request-01".to_owned(),
        idempotency_key: id.to_owned(),
        partition_key: format!("order/{id}"),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

pub fn envelope(definition: MessageDefinition, id: &str) -> Result<MessageEnvelope, TestError> {
    Ok(definition.build(metadata(definition, id), &json!({"mode": "dry_run"}))?)
}

// Own only successfully created resources. Never pre-delete a foreign stream.
// The timeout covers invocation of the body and its async work. Cleanup runs
// after success, error, timeout, and panic; disposable CI teardown covers abort.
pub async fn with_stream<F, Fut>(
    context: &jetstream::Context,
    config: Config,
    body: F,
) -> TestResult
where
    F: FnOnce(jetstream::stream::Stream) -> Fut,
    Fut: Future<Output = TestResult>,
{
    with_stream_deadline(context, config, DEADLINE, body).await
}

pub async fn with_stream_deadline<F, Fut>(
    context: &jetstream::Context,
    config: Config,
    case_deadline: Duration,
    body: F,
) -> TestResult
where
    F: FnOnce(jetstream::stream::Stream) -> Fut,
    Fut: Future<Output = TestResult>,
{
    let name = config.name.clone();
    let stream = tokio::time::timeout(DEADLINE, context.create_stream(config)).await??;
    let outcome =
        AssertUnwindSafe(async { tokio::time::timeout(case_deadline, body(stream)).await? })
            .catch_unwind()
            .await;
    let cleanup = async {
        if !tokio::time::timeout(DEADLINE, context.delete_stream(name))
            .await??
            .success
        {
            return Err("owned stream cleanup was not confirmed".into());
        }
        Ok::<(), TestError>(())
    }
    .await;
    match outcome {
        Ok(Err(error)) => {
            if cleanup.is_err() {
                eprintln!("owned stream cleanup also failed after proof error");
            }
            Err(error)
        }
        Ok(Ok(())) => cleanup,
        Err(panic) => {
            if cleanup.is_err() {
                eprintln!("owned stream cleanup failed after proof panic");
            }
            std::panic::resume_unwind(panic)
        }
    }
}
