//! Opt-in consumer conformance test against the checked-in local JetStream profile.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageEnvelope, MessageMetadata,
};
use edgeagent_messaging::{
    ConsumeErrorKind, DeliveryDisposition, MessageConsumer, MessagePublisher, PublishDisposition,
    RetryDelay,
};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use serde_json::json;
use std::env;
use std::error::Error;
use std::time::Duration;

const STREAM: &str = "EDGEAGENT_CONSUMER_CONFORMANCE";
const CONSUMER: &str = "execution_simulator_v1";
const OVERSIZE_STREAM: &str = "EDGEAGENT_OVERSIZE_CONFORMANCE";
const OVERSIZE_CONSUMER: &str = "execution_simulator_oversize_v1";
const OVERSIZE_SUBJECT: &str = "edgeagent.command.execution.oversized-raw.v1";
const ABANDONED_STREAM: &str = "EDGEAGENT_ABANDONED_SETTLEMENT_CONFORMANCE";
const ABANDONED_CONSUMER: &str = "execution_simulator_abandoned_v1";
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const ABANDONED_COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.abandoned-settlement.v1",
    "urn:edgeagent:schema:abandoned-settlement:v1",
    Component::ExecutionSimulator,
    "order",
);

fn envelope(
    definition: MessageDefinition,
    message_id: &str,
) -> Result<MessageEnvelope, Box<dyn Error>> {
    Ok(definition.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: definition.message_type().to_owned(),
            subject: format!("order/{message_id}"),
            time: "2026-09-28T00:00:00Z".to_owned(),
            data_schema: definition.data_schema().to_owned(),
            correlation_id: "consumer-correlation-01".to_owned(),
            causation_id: "consumer-request-01".to_owned(),
            idempotency_key: message_id.to_owned(),
            partition_key: format!("order/{message_id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run"}),
    )?)
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn delivery_settlement_controls_redelivery_and_acknowledgement() -> Result<(), Box<dyn Error>>
{
    let nats_url = env::var("EDGEAGENT_NATS_URL")?;
    let client = async_nats::connect(nats_url).await?;
    let context = jetstream::new(client);
    let stream = context
        .create_stream(Config {
            name: STREAM.to_owned(),
            subjects: vec![COMMAND.subject()?],
            storage: StorageType::Memory,
            ..Config::default()
        })
        .await?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER,
            pull::Config {
                durable_name: Some(CONSUMER.to_owned()),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(5),
                max_deliver: 5,
                max_ack_pending: 1,
                max_batch: 1,
                filter_subject: COMMAND.subject()?,
                ..pull::Config::default()
            },
        )
        .await?;
    let mut inspector = consumer.clone();
    let mut consumer = JetStreamConsumer::new(consumer).await?;
    let publisher = JetStreamPublisher::from_context(context.clone());

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
    let quarantined = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
    assert_eq!(quarantined.metadata().delivery_attempt(), 1);
    quarantined.settle(DeliveryDisposition::Quarantined).await?;

    let info = inspector.info().await?;
    assert_eq!(info.num_ack_pending, 0);
    assert_eq!(info.num_pending, 0);
    assert!(context.delete_stream(STREAM).await?.success);
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn abandoned_settlement_future_leaves_delivery_eligible_for_redelivery()
-> Result<(), Box<dyn Error>> {
    let nats_url = env::var("EDGEAGENT_NATS_URL")?;
    let client = async_nats::connect(nats_url).await?;
    let context = jetstream::new(client);
    let stream = context
        .create_stream(Config {
            name: ABANDONED_STREAM.to_owned(),
            subjects: vec![ABANDONED_COMMAND.subject()?],
            storage: StorageType::Memory,
            ..Config::default()
        })
        .await?;
    let durable = stream
        .get_or_create_consumer(
            ABANDONED_CONSUMER,
            pull::Config {
                durable_name: Some(ABANDONED_CONSUMER.to_owned()),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(1),
                max_deliver: 5,
                max_ack_pending: 1,
                max_batch: 1,
                filter_subject: ABANDONED_COMMAND.subject()?,
                ..pull::Config::default()
            },
        )
        .await?;
    let mut inspector = durable.clone();
    let mut consumer = JetStreamConsumer::new(durable).await?;
    let publisher = JetStreamPublisher::from_context(context.clone());
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

    let redelivered = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
    assert_eq!(redelivered.payload(), payload);
    assert_eq!(redelivered.metadata().message_key(), message_key);
    assert!(redelivered.metadata().delivery_attempt() >= 2);
    redelivered.settle(DeliveryDisposition::Acknowledge).await?;

    let info = inspector.info().await?;
    assert_eq!(info.num_ack_pending, 0);
    assert_eq!(info.num_pending, 0);
    assert!(context.delete_stream(ABANDONED_STREAM).await?.success);
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn oversized_raw_delivery_halts_intake_without_acknowledgement() -> Result<(), Box<dyn Error>>
{
    let nats_url = env::var("EDGEAGENT_NATS_URL")?;
    let client = async_nats::connect(nats_url).await?;
    let context = jetstream::new(client);
    let stream = context
        .create_stream(Config {
            name: OVERSIZE_STREAM.to_owned(),
            subjects: vec![OVERSIZE_SUBJECT.to_owned()],
            storage: StorageType::Memory,
            ..Config::default()
        })
        .await?;
    let durable = stream
        .get_or_create_consumer(
            OVERSIZE_CONSUMER,
            pull::Config {
                durable_name: Some(OVERSIZE_CONSUMER.to_owned()),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(1),
                max_deliver: 5,
                max_ack_pending: 1,
                max_batch: 1,
                filter_subject: OVERSIZE_SUBJECT.to_owned(),
                ..pull::Config::default()
            },
        )
        .await?;
    let mut consumer = JetStreamConsumer::new(durable.clone()).await?;

    context
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
    let redelivery = tokio::time::timeout(Duration::from_secs(8), replacement.receive()).await?;
    assert_eq!(
        redelivery.err().map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );

    assert!(context.delete_stream(OVERSIZE_STREAM).await?.success);
    Ok(())
}
