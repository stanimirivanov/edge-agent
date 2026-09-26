//! NATS JetStream implementation of the application-owned messaging port.

#![forbid(unsafe_code)]

use async_nats::jetstream;
use async_nats::jetstream::context::{
    PublishError as NatsPublishError, PublishErrorKind as NatsPublishErrorKind,
};
use async_nats::jetstream::message::PublishMessage;
use async_nats::jetstream::publish::PublishAck;
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use edgeagent_messaging::{
    MessagePublisher, PublishDisposition, PublishError, PublishErrorKind, PublishFuture,
    PublishReceipt,
};

/// Durable publisher backed by a configured NATS JetStream context.
///
/// Connection endpoints, credentials, TLS roots, reconnect behavior, and
/// workload identity remain composition-root concerns. This adapter owns only
/// portable contract enforcement and JetStream publication semantics.
#[derive(Clone, Debug)]
pub struct JetStreamPublisher {
    context: jetstream::Context,
}

impl JetStreamPublisher {
    /// Build a publisher from an application-configured NATS client.
    #[must_use]
    pub fn new(client: async_nats::Client) -> Self {
        Self::from_context(jetstream::new(client))
    }

    /// Build a publisher from an existing JetStream context.
    #[must_use]
    pub const fn from_context(context: jetstream::Context) -> Self {
        Self { context }
    }
}

impl MessagePublisher for JetStreamPublisher {
    fn publish<'publisher>(
        &'publisher self,
        definition: MessageDefinition,
        envelope: &'publisher MessageEnvelope,
    ) -> PublishFuture<'publisher> {
        let prepared = prepare_publish(definition, envelope).map_err(PublishError::from);

        Box::pin(async move {
            let prepared = prepared?;
            let publish = PublishMessage::build()
                .payload(prepared.payload.into())
                .message_id(prepared.message_id);
            let acknowledgement = self
                .context
                .send_publish(prepared.subject, publish)
                .await
                .map_err(map_send_error)?
                .await
                .map_err(map_acknowledgement_error)?;
            Ok(receipt(acknowledgement))
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct PreparedPublish {
    subject: String,
    message_id: String,
    payload: Vec<u8>,
}

fn prepare_publish(
    definition: MessageDefinition,
    envelope: &MessageEnvelope,
) -> Result<PreparedPublish, MessageRoutingError> {
    definition.validate_envelope(envelope)?;
    Ok(PreparedPublish {
        subject: definition.subject()?,
        message_id: envelope.deduplication_key(),
        payload: envelope.to_json()?,
    })
}

fn map_send_error(error: NatsPublishError) -> PublishError {
    let kind = match error.kind() {
        NatsPublishErrorKind::StreamNotFound
        | NatsPublishErrorKind::WrongLastMessageId
        | NatsPublishErrorKind::WrongLastSequence
        | NatsPublishErrorKind::MaxPayloadExceeded => PublishErrorKind::Rejected,
        NatsPublishErrorKind::TimedOut
        | NatsPublishErrorKind::BrokenPipe
        | NatsPublishErrorKind::MaxAckPending
        | NatsPublishErrorKind::Other => PublishErrorKind::Unavailable,
    };
    PublishError::with_source(kind, error)
}

fn map_acknowledgement_error(error: NatsPublishError) -> PublishError {
    let kind = match error.kind() {
        NatsPublishErrorKind::StreamNotFound
        | NatsPublishErrorKind::WrongLastMessageId
        | NatsPublishErrorKind::WrongLastSequence
        | NatsPublishErrorKind::MaxPayloadExceeded
        | NatsPublishErrorKind::MaxAckPending => PublishErrorKind::Rejected,
        NatsPublishErrorKind::TimedOut
        | NatsPublishErrorKind::BrokenPipe
        | NatsPublishErrorKind::Other => PublishErrorKind::ConfirmationUnknown,
    };
    PublishError::with_source(kind, error)
}

fn receipt(acknowledgement: PublishAck) -> PublishReceipt {
    if acknowledgement.duplicate {
        PublishReceipt::new(PublishDisposition::Duplicate)
    } else {
        PublishReceipt::new(PublishDisposition::Persisted)
    }
}

#[cfg(test)]
mod tests {
    use super::{map_acknowledgement_error, map_send_error, prepare_publish, receipt};
    use async_nats::jetstream::context::{PublishError, PublishErrorKind as NatsPublishErrorKind};
    use async_nats::jetstream::publish::PublishAck;
    use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRoutingError};
    use edgeagent_messaging::{PublishDisposition, PublishErrorKind};
    use serde_json::json;
    use std::error::Error;

    const COMMAND: MessageDefinition = MessageDefinition::command(
        "com.edgeagent.execution.submit-dry-run-order.v1",
        "urn:edgeagent:schema:submit-dry-run-order:v1",
        Component::ExecutionSimulator,
        "order",
    );

    fn envelope() -> Result<edgeagent_contracts::MessageEnvelope, MessageRoutingError> {
        COMMAND.build(
            MessageMetadata {
                id: "message-01".to_owned(),
                source: Component::Gateway.source_uri().to_owned(),
                message_type: COMMAND.message_type.to_owned(),
                subject: "order/order-01".to_owned(),
                time: "2026-09-26T00:00:00Z".to_owned(),
                data_schema: COMMAND.data_schema.to_owned(),
                correlation_id: "correlation-01".to_owned(),
                causation_id: "request-01".to_owned(),
                idempotency_key: "order-01".to_owned(),
                partition_key: "order/order-01".to_owned(),
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
                trace_state: None,
            },
            &json!({"mode": "dry_run"}),
        )
    }

    #[test]
    fn prepared_publication_derives_subject_identity_and_structured_payload()
    -> Result<(), Box<dyn Error>> {
        let envelope = envelope()?;
        let prepared = prepare_publish(COMMAND, &envelope)?;

        assert_eq!(
            prepared.subject,
            "edgeagent.command.execution.submit-dry-run-order.v1"
        );
        assert_eq!(
            prepared.message_id,
            "31:urn:edgeagent:component:gateway:message-01"
        );
        assert_eq!(prepared.payload, envelope.to_json()?);
        Ok(())
    }

    #[test]
    fn invalid_envelope_fails_before_transport_publication() -> Result<(), Box<dyn Error>> {
        let event_definition = MessageDefinition::event(
            COMMAND.message_type,
            COMMAND.data_schema,
            Component::ExecutionSimulator,
            COMMAND.partition_prefix,
            edgeagent_contracts::RetentionClass::WorkflowEvent,
        );
        let envelope = envelope()?;

        let result = prepare_publish(event_definition, &envelope);

        assert!(matches!(
            result,
            Err(MessageRoutingError::ContractMismatch {
                field: "source",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn send_and_acknowledgement_failures_have_distinct_retry_semantics() {
        let send_timeout = map_send_error(PublishError::new(NatsPublishErrorKind::TimedOut));
        let ack_timeout =
            map_acknowledgement_error(PublishError::new(NatsPublishErrorKind::TimedOut));
        let rejection =
            map_acknowledgement_error(PublishError::new(NatsPublishErrorKind::StreamNotFound));

        assert_eq!(send_timeout.kind(), PublishErrorKind::Unavailable);
        assert_eq!(ack_timeout.kind(), PublishErrorKind::ConfirmationUnknown);
        assert_eq!(rejection.kind(), PublishErrorKind::Rejected);
    }

    #[test]
    fn broker_duplicate_acknowledgement_is_portable() {
        let acknowledgement = PublishAck {
            duplicate: true,
            ..PublishAck::default()
        };

        assert_eq!(
            receipt(acknowledgement).disposition(),
            PublishDisposition::Duplicate
        );
    }
}
