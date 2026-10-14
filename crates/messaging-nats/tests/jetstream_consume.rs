//! Settlement and recovery conformance against the exclusive local profile.

pub mod support;

use async_nats::jetstream::consumer::{AckPolicy, pull};
use edgeagent_contracts::{Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition};
use edgeagent_messaging::{
    ConsumeErrorKind, DeliveryDisposition, MessageConsumer, MessagePublisher, PublishDisposition,
    RetryDelay,
};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use std::time::Duration;
use support::{
    TestResult, connect, consumer_config, envelope, resource, stream_config, with_stream,
};

const OVERSIZE_SUBJECT: &str = "edgeagent.command.execution.oversized-raw.v1";
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.adapter-consume-settlement.v1",
    "urn:edgeagent:schema:adapter-consume-settlement:v1",
    Component::ExecutionSimulator,
    "order",
);
const ABANDONED_COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.abandoned-settlement.v1",
    "urn:edgeagent:schema:abandoned-settlement:v1",
    Component::ExecutionSimulator,
    "order",
);

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn delivery_settlement_controls_redelivery_and_acknowledgement() -> TestResult {
    let context = connect().await?;
    let publication_context = context.clone();
    with_stream(
        &context,
        stream_config(&[COMMAND.subject()?.as_str()]),
        |stream| async move {
            let name = resource("consumer");
            let consumer = stream
                .create_consumer(pull::Config {
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(5),
                    max_deliver: 5,
                    max_ack_pending: 1,
                    max_batch: 1,
                    ..consumer_config(&name, &COMMAND.subject()?)
                })
                .await?;
            let mut inspector = consumer.clone();
            let mut consumer = JetStreamConsumer::new(consumer).await?;
            let publisher = JetStreamPublisher::from_context(publication_context.clone());

            let retried_envelope = envelope(COMMAND, "consumer-retry-01")?;
            assert_eq!(
                publisher
                    .publish(COMMAND, &retried_envelope)
                    .await?
                    .disposition(),
                PublishDisposition::Persisted
            );
            let first = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
            assert_eq!(first.payload(), retried_envelope.to_json()?);
            assert_eq!(first.metadata().delivery_attempt(), 1);
            assert_eq!(first.metadata().subject(), COMMAND.subject()?);
            let retried_message_key = first.metadata().message_key().to_owned();
            first
                .settle(DeliveryDisposition::RetryAfter(RetryDelay::new(
                    Duration::from_millis(1),
                )?))
                .await?;

            let retry = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
            assert_eq!(retry.payload(), retried_envelope.to_json()?);
            assert!(retry.metadata().delivery_attempt() >= 2);
            assert_eq!(retry.metadata().message_key(), retried_message_key);
            let retry_info = inspector.info().await?;
            assert_eq!(retry_info.num_ack_pending, 1);
            assert!(retry_info.num_redelivered >= 1);
            retry.settle(DeliveryDisposition::Acknowledge).await?;

            let quarantined_envelope = envelope(COMMAND, "consumer-quarantine-01")?;
            assert_eq!(
                publisher
                    .publish(COMMAND, &quarantined_envelope)
                    .await?
                    .disposition(),
                PublishDisposition::Persisted
            );
            let quarantined =
                tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
            assert_eq!(quarantined.metadata().delivery_attempt(), 1);
            quarantined.settle(DeliveryDisposition::Quarantined).await?;

            let info = inspector.info().await?;
            assert_eq!(info.num_ack_pending, 0);
            assert_eq!(info.num_pending, 0);
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn abandoned_settlement_future_leaves_delivery_eligible_for_redelivery() -> TestResult {
    let context = connect().await?;
    let publication_context = context.clone();
    with_stream(
        &context,
        stream_config(&[ABANDONED_COMMAND.subject()?.as_str()]),
        |stream| async move {
            let name = resource("consumer");
            let durable = stream
                .create_consumer(pull::Config {
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(1),
                    max_deliver: 5,
                    max_ack_pending: 1,
                    max_batch: 1,
                    ..consumer_config(&name, &ABANDONED_COMMAND.subject()?)
                })
                .await?;
            let mut inspector = durable.clone();
            let mut consumer = JetStreamConsumer::new(durable).await?;
            let publisher = JetStreamPublisher::from_context(publication_context.clone());
            let envelope = envelope(ABANDONED_COMMAND, "consumer-abandoned-01")?;
            let payload = envelope.to_json()?;
            assert_eq!(
                publisher
                    .publish(ABANDONED_COMMAND, &envelope)
                    .await?
                    .disposition(),
                PublishDisposition::Persisted
            );

            let first = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
            assert_eq!(first.payload(), payload);
            assert_eq!(first.metadata().delivery_attempt(), 1);
            let message_key = first.metadata().message_key().to_owned();
            // The NATS adapter sends its acknowledgement inside the future, not at construction.
            let abandoned = first.settle(DeliveryDisposition::Acknowledge);
            drop(abandoned);

            let redelivered =
                tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
            assert_eq!(redelivered.payload(), payload);
            assert_eq!(redelivered.metadata().message_key(), message_key);
            assert!(redelivered.metadata().delivery_attempt() >= 2);
            redelivered.settle(DeliveryDisposition::Acknowledge).await?;

            let info = inspector.info().await?;
            assert_eq!(info.num_ack_pending, 0);
            assert_eq!(info.num_pending, 0);
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn oversized_raw_delivery_halts_intake_without_acknowledgement() -> TestResult {
    let context = connect().await?;
    let publication_context = context.clone();
    with_stream(
        &context,
        stream_config(&[OVERSIZE_SUBJECT]),
        |stream| async move {
            let name = resource("consumer");
            let durable = stream
                .create_consumer(pull::Config {
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(1),
                    max_deliver: 5,
                    max_ack_pending: 1,
                    max_batch: 1,
                    ..consumer_config(&name, OVERSIZE_SUBJECT)
                })
                .await?;
            let mut consumer = JetStreamConsumer::new(durable.clone()).await?;

            publication_context
                .publish(
                    OVERSIZE_SUBJECT.to_owned(),
                    vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES + 1].into(),
                )
                .await?
                .await?;

            let first = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await?;
            assert_eq!(
                first.err().map(|error| error.kind()),
                Some(ConsumeErrorKind::Protocol)
            );
            let second = tokio::time::timeout(Duration::from_secs(1), consumer.receive()).await?;
            assert_eq!(
                second.err().map(|error| error.kind()),
                Some(ConsumeErrorKind::Protocol)
            );
            drop(consumer);
            let mut replacement = JetStreamConsumer::new(durable).await?;
            let redelivery =
                tokio::time::timeout(Duration::from_secs(8), replacement.receive()).await?;
            assert_eq!(
                redelivery.err().map(|error| error.kind()),
                Some(ConsumeErrorKind::Protocol)
            );

            Ok(())
        },
    )
    .await
}
