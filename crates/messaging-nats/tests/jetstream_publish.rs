//! Immutable publication identity conformance against the local profile.

pub mod support;

use edgeagent_contracts::{Component, MessageDefinition};
use edgeagent_messaging::{MessagePublisher, PublishDisposition};
use edgeagent_messaging_nats::JetStreamPublisher;
use std::time::Duration;
use support::{TestResult, connect, envelope, stream_config, with_stream};

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.adapter-publish-retry.v1",
    "urn:edgeagent:schema:adapter-publish-retry:v1",
    Component::ExecutionSimulator,
    "order",
);

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn persisted_message_identity_deduplicates_on_retry() -> TestResult {
    let context = connect().await?;
    let publication_context = context.clone();
    let mut config = stream_config(&[COMMAND.subject()?.as_str()]);
    config.duplicate_window = Duration::from_secs(120);
    with_stream(&context, config, |_| async move {
        let publisher = JetStreamPublisher::from_context(publication_context);
        let envelope = envelope(COMMAND, "conformance-message-01")?;
        let first = publisher.publish(COMMAND, &envelope).await?;
        let retry = publisher.publish(COMMAND, &envelope).await?;
        assert_eq!(first.disposition(), PublishDisposition::Persisted);
        assert_eq!(retry.disposition(), PublishDisposition::Duplicate);
        Ok(())
    })
    .await
}
