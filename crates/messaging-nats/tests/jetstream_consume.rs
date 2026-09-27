//! Opt-in consumer conformance test against the checked-in local JetStream profile.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{Component, MessageDefinition, MessageEnvelope, MessageMetadata};
use edgeagent_messaging::{
    DeliveryDisposition, MessageConsumer, MessagePublisher, PublishDisposition,
};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use serde_json::json;
use std::env;
use std::error::Error;
use std::time::Duration;

const STREAM: &str = "EDGEAGENT_CONSUMER_CONFORMANCE";
const CONSUMER: &str = "execution_simulator_v1";
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn envelope(message_id: &str) -> Result<MessageEnvelope, Box<dyn Error>> {
    Ok(COMMAND.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type.to_owned(),
            subject: format!("order/{message_id}"),
            time: "2026-09-28T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema.to_owned(),
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

    let retried_envelope = envelope("consumer-retry-01")?;
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
    first
        .settle(DeliveryDisposition::RetryAfter(Duration::from_millis(1)))
        .await?;

    let retry = tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
    assert_eq!(retry.payload(), retried_envelope.to_json()?);
    assert!(retry.metadata().delivery_attempt() >= 2);
    let retry_info = inspector.info().await?;
    assert_eq!(retry_info.num_ack_pending, 1);
    assert!(retry_info.num_redelivered >= 1);
    retry.settle(DeliveryDisposition::Acknowledge).await?;

    let quarantined_envelope = envelope("consumer-quarantine-01")?;
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
