//! Opt-in broker-boundary regressions against the isolated local NATS profile.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageEnvelope, MessageMetadata,
};
use edgeagent_messaging::{
    ConsumeErrorKind, DeliveryDisposition, MessageConsumer, MessagePublisher, PublishErrorKind,
    RetryDelay,
};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use futures_util::{FutureExt, StreamExt};
use serde_json::json;
use std::error::Error;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

type TestError = Box<dyn Error + Send + Sync>;
type TestResult = Result<(), TestError>;
const DEADLINE: Duration = Duration::from_secs(10);
static NEXT_RESOURCE: AtomicU64 = AtomicU64::new(1);

const NO_STREAM: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.boundary-no-stream.v1",
    "urn:edgeagent:schema:boundary-no-stream:v1",
    Component::ExecutionSimulator,
    "order",
);
const SIZE_REJECTION: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.boundary-size-rejection.v1",
    "urn:edgeagent:schema:boundary-size-rejection:v1",
    Component::ExecutionSimulator,
    "order",
);
const HEADERS_ONLY: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.boundary-headers-only.v1",
    "urn:edgeagent:schema:boundary-headers-only:v1",
    Component::ExecutionSimulator,
    "order",
);
const OVERSIZE: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.boundary-publish-oversize.v1",
    "urn:edgeagent:schema:boundary-publish-oversize:v1",
    Component::ExecutionSimulator,
    "order",
);
const SETTLEMENT: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.boundary-settlement-response.v1",
    "urn:edgeagent:schema:boundary-settlement-response:v1",
    Component::ExecutionSimulator,
    "order",
);

fn resource(prefix: &str) -> String {
    format!(
        "{prefix}_{}_{}",
        std::process::id(),
        NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed)
    )
}

async fn connect() -> Result<jetstream::Context, TestError> {
    let url = std::env::var("EDGEAGENT_NATS_URL")?;
    let client = tokio::time::timeout(DEADLINE, async_nats::connect(url)).await??;
    Ok(jetstream::ContextBuilder::new()
        .timeout(DEADLINE)
        .build(client))
}

fn metadata(definition: MessageDefinition) -> MessageMetadata {
    MessageMetadata {
        id: "boundary-message-01".to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: definition.message_type().to_owned(),
        subject: "order/boundary-01".to_owned(),
        time: "2026-10-13T00:00:00Z".to_owned(),
        data_schema: definition.data_schema().to_owned(),
        correlation_id: "boundary-correlation-01".to_owned(),
        causation_id: "boundary-request-01".to_owned(),
        idempotency_key: "boundary-order-01".to_owned(),
        partition_key: "order/boundary-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

fn envelope(definition: MessageDefinition) -> Result<MessageEnvelope, TestError> {
    Ok(definition.build(metadata(definition), &json!({"mode": "dry_run"}))?)
}

// Own only the successfully created, process-scoped stream. Never pre-delete
// another run's resource. CI's disposable broker covers task/process abort.
async fn with_stream<F, Fut>(context: &jetstream::Context, config: Config, body: F) -> TestResult
where
    F: FnOnce(jetstream::stream::Stream) -> Fut,
    Fut: Future<Output = TestResult>,
{
    let name = config.name.clone();
    let stream = tokio::time::timeout(DEADLINE, context.create_stream(config)).await??;
    let outcome = AssertUnwindSafe(async { tokio::time::timeout(DEADLINE, body(stream)).await? })
        .catch_unwind()
        .await;
    let cleanup = async {
        let deleted = tokio::time::timeout(DEADLINE, context.delete_stream(name)).await??;
        if !deleted.success {
            return Err("owned stream cleanup was not confirmed".into());
        }
        Ok::<(), TestError>(())
    }
    .await;
    match outcome {
        Ok(result) => {
            cleanup?;
            result
        }
        Err(panic) => {
            if cleanup.is_err() {
                eprintln!("owned stream cleanup failed after proof panic");
            }
            std::panic::resume_unwind(panic)
        }
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn publication_without_an_owning_stream_is_rejected() -> TestResult {
    let context = connect().await?;
    let publisher = JetStreamPublisher::from_context(context);
    let error = publisher
        .publish(NO_STREAM, &envelope(NO_STREAM)?)
        .await
        .err()
        .ok_or("no-stream publication must fail")?;
    assert_eq!(error.kind(), PublishErrorKind::Rejected);
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn stream_message_limit_is_classified_as_rejected() -> TestResult {
    let context = connect().await?;
    let publication_context = context.clone();
    with_stream(
        &context,
        Config {
            name: resource("EDGEAGENT_SIZE_REJECTION"),
            subjects: vec![SIZE_REJECTION.subject()?],
            storage: StorageType::Memory,
            max_message_size: 8,
            ..Config::default()
        },
        |stream| async move {
            let publisher = JetStreamPublisher::from_context(publication_context);
            let error = publisher
                .publish(SIZE_REJECTION, &envelope(SIZE_REJECTION)?)
                .await
                .err()
                .ok_or("stream size limit must reject publication")?;
            assert_eq!(error.kind(), PublishErrorKind::Rejected);
            let cause = error
                .source()
                .and_then(|source| source.source())
                .and_then(|source| source.downcast_ref::<jetstream::Error>())
                .ok_or("typed JetStream rejection must be preserved")?;
            assert_eq!(
                cause.error_code(),
                jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM
            );
            assert_eq!(stream.get_info().await?.state.messages, 0);
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn header_only_consumer_is_rejected_without_starting_intake() -> TestResult {
    let context = connect().await?;
    with_stream(
        &context,
        Config {
            name: resource("EDGEAGENT_HEADERS_ONLY"),
            subjects: vec![HEADERS_ONLY.subject()?],
            storage: StorageType::Memory,
            ..Config::default()
        },
        |stream| async move {
            let name = resource("headers_only");
            let durable = stream
                .create_consumer(pull::Config {
                    durable_name: Some(name),
                    ack_policy: AckPolicy::Explicit,
                    headers_only: true,
                    ..pull::Config::default()
                })
                .await?;
            let inspector = durable.clone();
            let error = JetStreamConsumer::new(durable)
                .await
                .err()
                .ok_or("headers-only consumer must fail")?;
            assert_eq!(error.kind(), ConsumeErrorKind::Protocol);
            let info = inspector.get_info().await?;
            assert_eq!(info.num_waiting, 0);
            assert_eq!(info.delivered.consumer_sequence, 0);
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn oversized_publication_never_reaches_transport() -> TestResult {
    let context = connect().await?;
    let client = context.client();
    let mut observer = client.subscribe(OVERSIZE.subject()?).await?;
    client.flush().await?;
    let publisher = JetStreamPublisher::from_context(context);
    let envelope = MessageEnvelope::from_payload(
        metadata(OVERSIZE),
        &json!({"padding": "x".repeat(MAX_PORTABLE_MESSAGE_BYTES)}),
    )?;
    let error = publisher
        .publish(OVERSIZE, &envelope)
        .await
        .err()
        .ok_or("oversize publication must fail")?;
    assert_eq!(error.kind(), PublishErrorKind::Contract);
    // The flush orders broker processing before observing the subscription;
    // no sleep or absence inferred from an arbitrary timeout is needed.
    client.flush().await?;
    assert!(observer.next().now_or_never().is_none());
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn non_success_settlement_response_is_not_confirmation() -> TestResult {
    let context = connect().await?;
    for disposition in [
        DeliveryDisposition::Acknowledge,
        DeliveryDisposition::Quarantined,
        DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(1))?),
    ] {
        let publication_context = context.clone();
        with_stream(
            &context,
            Config {
                name: resource("EDGEAGENT_SETTLEMENT_RESPONSE"),
                subjects: vec![SETTLEMENT.subject()?],
                storage: StorageType::Memory,
                ..Config::default()
            },
            |stream| async move {
                let name = resource("settlement_response");
                let durable = stream
                    .create_consumer(pull::Config {
                        durable_name: Some(name.clone()),
                        ack_policy: AckPolicy::Explicit,
                        ..pull::Config::default()
                    })
                    .await?;
                let publisher = JetStreamPublisher::from_context(publication_context);
                publisher
                    .publish(SETTLEMENT, &envelope(SETTLEMENT)?)
                    .await?;
                let mut consumer = JetStreamConsumer::new(durable).await?;
                let delivery = tokio::time::timeout(DEADLINE, consumer.receive()).await??;
                // Delete the real durable consumer after receipt. The old ACK
                // subject no longer has a responder; 503 is not confirmation.
                assert!(stream.delete_consumer(&name).await?.success);
                let error = tokio::time::timeout(DEADLINE, delivery.settle(disposition))
                    .await?
                    .err()
                    .ok_or("no responder must not confirm settlement")?;
                assert_eq!(error.kind(), ConsumeErrorKind::ConfirmationUnknown);
                let cause = error
                    .source()
                    .and_then(|source| source.downcast_ref::<async_nats::RequestError>())
                    .ok_or("request error must be preserved")?;
                assert_eq!(cause.kind(), async_nats::RequestErrorKind::NoResponders);
                Ok(())
            },
        )
        .await?;
    }
    Ok(())
}
