//! Broker-boundary regressions against the exclusive local NATS profile.

pub mod support;

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageEnvelope,
};
use edgeagent_messaging::{
    ConsumeErrorKind, DeliveryDisposition, MessageConsumer, MessagePublisher, PublishErrorKind,
    RetryDelay,
};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use futures_util::{FutureExt, StreamExt};
use serde_json::json;
use std::error::Error;
use std::panic::AssertUnwindSafe;
use std::time::Duration;
use support::{
    DEADLINE, TestResult, connect, envelope, metadata, resource, stream_config, with_stream,
    with_stream_deadline,
};

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

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn publication_without_an_owning_stream_is_rejected() -> TestResult {
    let context = connect().await?;
    let publisher = JetStreamPublisher::from_context(context);
    let error = publisher
        .publish(NO_STREAM, &envelope(NO_STREAM, "boundary-message-01")?)
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
                .publish(
                    SIZE_REJECTION,
                    &envelope(SIZE_REJECTION, "boundary-message-01")?,
                )
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
        metadata(OVERSIZE, "boundary-message-01"),
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
                    .publish(SETTLEMENT, &envelope(SETTLEMENT, "boundary-message-01")?)
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

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
#[expect(
    clippy::panic,
    reason = "Inject fixture panics to prove cleanup before unwind resumes"
)]
async fn owned_stream_cleanup_runs_after_failure_panic_and_timeout() -> TestResult {
    let context = connect().await?;
    for exit in 0..4 {
        let config = stream_config(&["edgeagent.command.execution.fixture-cleanup.v1"]);
        let name = config.name.clone();
        let deadline = if exit == 3 { Duration::ZERO } else { DEADLINE };
        let outcome = AssertUnwindSafe(with_stream_deadline(&context, config, deadline, |_| {
            if exit == 1 {
                panic!("synthetic synchronous fixture panic");
            }
            async move {
                if exit == 2 {
                    panic!("synthetic asynchronous fixture panic");
                }
                if exit == 3 {
                    return std::future::pending::<TestResult>().await;
                }
                Err("synthetic fixture failure".into())
            }
        }))
        .catch_unwind()
        .await;
        if matches!(exit, 1 | 2) {
            assert!(outcome.is_err());
        } else {
            assert!(outcome.is_ok_and(|result| result.is_err()));
        }
        let error = context
            .get_stream(name)
            .await
            .err()
            .ok_or("owned stream must be deleted")?;
        assert!(
            matches!(error.kind(), jetstream::context::GetStreamErrorKind::JetStream(error) if error.error_code() == jetstream::ErrorCode::STREAM_NOT_FOUND)
        );
    }
    Ok(())
}
