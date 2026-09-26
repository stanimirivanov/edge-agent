//! Opt-in conformance test against the checked-in local JetStream profile.

use async_nats::jetstream;
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata};
use edgeagent_messaging::{MessagePublisher, PublishDisposition};
use edgeagent_messaging_nats::JetStreamPublisher;
use serde_json::json;
use std::env;
use std::error::Error;
use std::time::Duration;

const STREAM: &str = "EDGEAGENT_ADAPTER_CONFORMANCE";
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn persisted_message_identity_deduplicates_on_retry() -> Result<(), Box<dyn Error>> {
    let nats_url = env::var("EDGEAGENT_NATS_URL")?;
    let client = async_nats::connect(nats_url).await?;
    let context = jetstream::new(client);
    context
        .create_stream(Config {
            name: STREAM.to_owned(),
            subjects: vec![COMMAND.subject()?],
            storage: StorageType::Memory,
            duplicate_window: Duration::from_secs(120),
            ..Config::default()
        })
        .await?;

    let publisher = JetStreamPublisher::from_context(context.clone());
    let envelope = COMMAND.build(
        MessageMetadata {
            id: "conformance-message-01".to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type.to_owned(),
            subject: "order/conformance-order-01".to_owned(),
            time: "2026-09-26T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema.to_owned(),
            correlation_id: "conformance-correlation-01".to_owned(),
            causation_id: "conformance-request-01".to_owned(),
            idempotency_key: "conformance-order-01".to_owned(),
            partition_key: "order/conformance-order-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run"}),
    )?;

    let first = publisher.publish(COMMAND, &envelope).await?;
    let retry = publisher.publish(COMMAND, &envelope).await?;

    assert_eq!(first.disposition(), PublishDisposition::Persisted);
    assert_eq!(retry.disposition(), PublishDisposition::Duplicate);
    assert!(context.delete_stream(STREAM).await?.success);
    Ok(())
}
