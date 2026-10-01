//! NATS JetStream implementation of the application-owned messaging port.

#![forbid(unsafe_code)]

use async_nats::jetstream;
use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::pull;
use async_nats::jetstream::consumer::{AckPolicy, PullConsumer};
use async_nats::jetstream::context::{
    PublishError as NatsPublishError, PublishErrorKind as NatsPublishErrorKind,
};
use async_nats::jetstream::message::Acker;
use async_nats::jetstream::message::PublishMessage;
use async_nats::jetstream::publish::PublishAck;
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliveryMetadata, DeliverySettlement,
    MessageConsumer, MessageDelivery, MessagePublisher, PublishDisposition, PublishError,
    PublishErrorKind, PublishFuture, PublishReceipt, ReceiveFuture, SettlementFuture,
};
use futures_util::StreamExt;
use std::time::Duration;

const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

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

/// Pull-based JetStream consumer behind the portable one-delivery port.
///
/// The supplied consumer must be durable, use explicit acknowledgements, and
/// be provisioned with application-compatible subject, retention, maximum
/// delivery, acknowledgement wait, and pending limits. Provisioning remains a
/// deployment concern. This adapter bounds client-side prefetch to one message.
pub struct JetStreamConsumer {
    messages: pull::Stream,
}

impl JetStreamConsumer {
    /// Start a bounded pull stream from an existing configured consumer.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the initial pull stream cannot be established.
    pub async fn new(consumer: PullConsumer) -> Result<Self, ConsumeError> {
        validate_consumer_configuration(&consumer.cached_info().config)?;
        let messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Unavailable, error))?;
        Ok(Self { messages })
    }
}

fn validate_consumer_configuration(
    config: &jetstream::consumer::Config,
) -> Result<(), ConsumeError> {
    if config.durable_name.is_none() {
        return Err(ConsumeError::protocol(
            "consumer must have a durable name for restart recovery",
        ));
    }
    if config.ack_policy != AckPolicy::Explicit {
        return Err(ConsumeError::protocol(
            "consumer must use explicit acknowledgement policy",
        ));
    }
    Ok(())
}

impl MessageConsumer for JetStreamConsumer {
    fn receive(&mut self) -> ReceiveFuture<'_> {
        Box::pin(async move {
            let message = self
                .messages
                .next()
                .await
                .ok_or_else(|| ConsumeError::unavailable("consumer stream ended"))?
                .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Unavailable, error))?;
            let info = message.info().map_err(|error| {
                ConsumeError::with_boxed_source(ConsumeErrorKind::Protocol, error)
            })?;
            let delivery_attempt = u32::try_from(info.delivered)
                .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Protocol, error))?;
            let message_key = delivery_message_key(info.stream, info.stream_sequence)?;
            let metadata = DeliveryMetadata::new(
                message_key,
                message.subject.to_string(),
                delivery_attempt,
                info.pending,
                info.stream_sequence,
                info.consumer_sequence,
            )?;
            let payload = message.payload.to_vec();
            let (_, acker) = message.split();
            Ok(MessageDelivery::new(
                payload,
                metadata,
                Box::new(JetStreamSettlement { acker }),
            ))
        })
    }
}

fn delivery_message_key(stream: &str, stream_sequence: u64) -> Result<String, ConsumeError> {
    if stream.is_empty() || stream_sequence == 0 {
        return Err(ConsumeError::protocol(
            "stream identity must contain a name and positive sequence",
        ));
    }
    Ok(format!("{}:{stream}:{stream_sequence}", stream.len()))
}

struct JetStreamSettlement {
    acker: Acker,
}

impl DeliverySettlement for JetStreamSettlement {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async move {
            let kind = acknowledgement_kind(disposition)?;
            self.acker.double_ack_with(kind).await.map_err(|error| {
                ConsumeError::with_boxed_source(ConsumeErrorKind::ConfirmationUnknown, error)
            })
        })
    }
}

fn acknowledgement_kind(disposition: DeliveryDisposition) -> Result<AckKind, ConsumeError> {
    match disposition {
        DeliveryDisposition::Acknowledge => Ok(AckKind::Ack),
        DeliveryDisposition::Quarantined => Ok(AckKind::Term),
        DeliveryDisposition::RetryAfter(delay)
            if delay >= Duration::from_millis(1) && delay <= MAX_RETRY_DELAY =>
        {
            Ok(AckKind::Nak(Some(delay)))
        }
        DeliveryDisposition::RetryAfter(_) => Err(ConsumeError::invalid_disposition(
            "retry delay must be between 1 millisecond and 24 hours",
        )),
    }
}

#[derive(Eq, PartialEq)]
struct PreparedPublish {
    subject: String,
    message_id: String,
    payload: Vec<u8>,
}

impl std::fmt::Debug for PreparedPublish {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPublish")
            .field("subject", &self.subject)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
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
    use super::{
        PreparedPublish, acknowledgement_kind, delivery_message_key, map_acknowledgement_error,
        map_send_error, prepare_publish, receipt, validate_consumer_configuration,
    };
    use async_nats::jetstream::AckKind;
    use async_nats::jetstream::consumer::{AckPolicy, Config as ConsumerConfig};
    use async_nats::jetstream::context::{PublishError, PublishErrorKind as NatsPublishErrorKind};
    use async_nats::jetstream::publish::PublishAck;
    use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata, MessageRoutingError};
    use edgeagent_messaging::{
        ConsumeErrorKind, DeliveryDisposition, PublishDisposition, PublishErrorKind,
    };
    use serde_json::json;
    use std::error::Error;
    use std::time::Duration;

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
    fn prepared_publication_debug_omits_payload_bytes() {
        let payload = b"private-publication-sentinel-7391";
        let prepared = PreparedPublish {
            subject: "events.subject".to_owned(),
            message_id: "message-01".to_owned(),
            payload: payload.to_vec(),
        };

        let rendered = format!("{prepared:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-publication-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(prepared.payload, payload);
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

    #[test]
    fn settlement_maps_to_confirmed_acknowledgement_kinds() {
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::Acknowledge),
            Ok(AckKind::Ack)
        ));
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::Quarantined),
            Ok(AckKind::Term)
        ));
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::RetryAfter(Duration::from_secs(2))),
            Ok(AckKind::Nak(Some(delay))) if delay == Duration::from_secs(2)
        ));
        assert_eq!(
            acknowledgement_kind(DeliveryDisposition::RetryAfter(Duration::ZERO))
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::InvalidDisposition)
        );
    }

    #[test]
    fn consumer_requires_durable_explicit_acknowledgement() {
        assert_eq!(
            validate_consumer_configuration(&ConsumerConfig::default())
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        let durable_without_ack = ConsumerConfig {
            durable_name: Some("execution_simulator_v1".to_owned()),
            ack_policy: AckPolicy::None,
            ..ConsumerConfig::default()
        };
        assert_eq!(
            validate_consumer_configuration(&durable_without_ack)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        let valid = ConsumerConfig {
            durable_name: Some("execution_simulator_v1".to_owned()),
            ack_policy: AckPolicy::Explicit,
            ..ConsumerConfig::default()
        };
        assert!(validate_consumer_configuration(&valid).is_ok());
    }

    #[test]
    fn delivery_message_keys_are_stable_and_unambiguous() {
        assert_eq!(
            delivery_message_key("ORDERS", 41).ok().as_deref(),
            Some("6:ORDERS:41")
        );
        assert_eq!(
            delivery_message_key("", 41).err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
    }
}
