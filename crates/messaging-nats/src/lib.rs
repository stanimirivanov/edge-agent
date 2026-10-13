//! NATS JetStream implementation of the application-owned messaging port.

#![forbid(unsafe_code)]

use async_nats::jetstream;
use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::pull;
use async_nats::jetstream::consumer::{AckPolicy, PullConsumer};
use async_nats::jetstream::context::traits::TimeoutProvider;
use async_nats::jetstream::context::{
    PublishError as NatsPublishError, PublishErrorKind as NatsPublishErrorKind,
};
use async_nats::jetstream::message::PublishMessage;
use async_nats::jetstream::publish::PublishAck;
use async_nats::{Request, StatusCode};
use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey,
    DeliveryMetadata, DeliverySettlement, DeliverySubject, MessageConsumer, MessageDelivery,
    MessagePublisher, PublishDisposition, PublishError, PublishErrorKind, PublishFuture,
    PublishReceipt, ReceiveFuture, SettlementFuture,
};
use futures_util::StreamExt;
use std::error::Error;
use std::time::Duration;

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
/// The supplied consumer must be durable, deliver complete message bodies, and
/// use explicit acknowledgements. Provision application-compatible subject, retention, maximum
/// delivery, acknowledgement wait, and pending limits. Provisioning remains a
/// deployment concern. This adapter bounds client-side prefetch to one message.
/// Invalid broker metadata or a payload outside portable bounds leaves the raw
/// message unsettled and halts this instance before another pull; replacement
/// requires operator investigation.
/// Terminal states release the SDK stream and abort its background pull worker.
/// Already-issued requests are not revoked. Stream closure and terminal SDK
/// errors close intake; transient pull errors preserve it. Cancelling a pending
/// receive leaves the stream active and does not acknowledge any delivery.
pub struct JetStreamConsumer {
    intake: ConsumerIntake<pull::Stream>,
}

// Terminal states cannot retain a stream: dropping the SDK stream aborts its
// expiry/reconnect worker, even while the adapter itself remains alive.
enum ConsumerIntake<S> {
    Active(S),
    Halted,
    Closed,
}

impl<S> ConsumerIntake<S> {
    fn stream(&mut self) -> Result<&mut S, ConsumeError> {
        match self {
            Self::Active(stream) => Ok(stream),
            Self::Halted => Err(ConsumeError::protocol(
                "consumer halted after invalid broker delivery",
            )),
            Self::Closed => Err(ConsumeError::unavailable("consumer stream ended")),
        }
    }

    fn resolve_pull<T>(
        &mut self,
        result: Option<Result<T, pull::MessagesError>>,
    ) -> Result<T, ConsumeError> {
        match result {
            Some(Ok(message)) => Ok(message),
            Some(Err(error)) => {
                match error.kind() {
                    pull::MessagesErrorKind::ConsumerDeleted
                    | pull::MessagesErrorKind::PushBasedConsumer => *self = Self::Closed,
                    pull::MessagesErrorKind::MissingHeartbeat
                    | pull::MessagesErrorKind::Pull
                    | pull::MessagesErrorKind::NoResponders
                    | pull::MessagesErrorKind::Other => (),
                }
                Err(ConsumeError::with_source(
                    ConsumeErrorKind::Unavailable,
                    error,
                ))
            }
            None => {
                *self = Self::Closed;
                Err(ConsumeError::unavailable("consumer stream ended"))
            }
        }
    }

    fn validate<T>(&mut self, result: Result<T, ConsumeError>) -> Result<T, ConsumeError> {
        if result.is_err() {
            *self = Self::Halted;
        }
        result
    }
}

impl JetStreamConsumer {
    /// Start a bounded pull stream from an existing configured consumer.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for incompatible cached consumer configuration, or
    /// `Unavailable` when the initial pull stream cannot be established.
    /// Configuration must remain unchanged while this adapter is active;
    /// quiesce and recreate the adapter after provisioning changes.
    pub async fn new(consumer: PullConsumer) -> Result<Self, ConsumeError> {
        validate_consumer_configuration(&consumer.cached_info().config)?;
        let messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Unavailable, error))?;
        Ok(Self {
            intake: ConsumerIntake::Active(messages),
        })
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
    if config.headers_only {
        return Err(ConsumeError::protocol(
            "consumer must deliver complete message bodies",
        ));
    }
    Ok(())
}

impl MessageConsumer for JetStreamConsumer {
    fn receive(&mut self) -> ReceiveFuture<'_> {
        Box::pin(async move {
            // Borrow rather than move the stream across await. Cancellation
            // while waiting leaves intake active; after receipt validation and
            // ownership transfer are synchronous, with no cancellation gap.
            let result = self.intake.stream()?.next().await;
            let message = self.intake.resolve_pull(result)?;
            let metadata = self.intake.validate((|| {
                let info = message.info().map_err(|error| {
                    ConsumeError::with_boxed_source(ConsumeErrorKind::Protocol, error)
                })?;
                let delivery_attempt =
                    DeliveryAttempt::new(u32::try_from(info.delivered).map_err(|error| {
                        ConsumeError::with_source(ConsumeErrorKind::Protocol, error)
                    })?)?;
                let message_key = DeliveryMessageKey::new(delivery_message_key(
                    info.stream,
                    info.stream_sequence,
                )?)?;
                validate_consumer_sequence(info.consumer_sequence)?;
                Ok(DeliveryMetadata::new(
                    message_key,
                    DeliverySubject::new(message.subject.to_string())?,
                    delivery_attempt,
                ))
            })())?;
            let jetstream::Message { message, context } = message;
            let reply = self.intake.validate(message.reply.ok_or_else(|| {
                ConsumeError::protocol("delivery must contain a settlement reply subject")
            }))?;
            self.intake.validate(MessageDelivery::new(
                message.payload,
                metadata,
                Box::new(JetStreamSettlement {
                    client: context.client(),
                    reply,
                    timeout: context.timeout(),
                }),
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

fn validate_consumer_sequence(sequence: u64) -> Result<(), ConsumeError> {
    if sequence == 0 {
        Err(ConsumeError::protocol(
            "consumer sequence must be greater than zero",
        ))
    } else {
        Ok(())
    }
}

struct JetStreamSettlement {
    client: async_nats::Client,
    reply: async_nats::Subject,
    timeout: Duration,
}

impl DeliverySettlement for JetStreamSettlement {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async move {
            // The pinned SDK's double_ack_with accepts any reply. Keep its
            // two-way protocol, but require a successful empty confirmation.
            let response = self
                .client
                .send_request(
                    self.reply,
                    Request::new()
                        .payload(acknowledgement_kind(disposition).into())
                        .timeout(Some(self.timeout)),
                )
                .await
                .map_err(|error| {
                    ConsumeError::with_source(ConsumeErrorKind::ConfirmationUnknown, error)
                })?;
            validate_settlement_confirmation(&response)
        })
    }
}

#[derive(Debug)]
struct InvalidSettlementConfirmation {
    status: Option<u16>,
    payload_bytes: usize,
}

impl std::fmt::Display for InvalidSettlementConfirmation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid JetStream settlement confirmation (status {:?}, payload bytes {})",
            self.status, self.payload_bytes
        )
    }
}

impl Error for InvalidSettlementConfirmation {}

fn validate_settlement_confirmation(response: &async_nats::Message) -> Result<(), ConsumeError> {
    if response
        .status
        .is_some_and(|status| status != StatusCode::OK)
        || !response.payload.is_empty()
    {
        return Err(ConsumeError::with_source(
            ConsumeErrorKind::ConfirmationUnknown,
            InvalidSettlementConfirmation {
                status: response.status.map(|status| status.as_u16()),
                payload_bytes: response.payload.len(),
            },
        ));
    }
    Ok(())
}

fn acknowledgement_kind(disposition: DeliveryDisposition) -> AckKind {
    match disposition {
        DeliveryDisposition::Acknowledge => AckKind::Ack,
        DeliveryDisposition::Quarantined => AckKind::Term,
        DeliveryDisposition::RetryAfter(delay) => AckKind::Nak(Some(delay.get())),
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
        NatsPublishErrorKind::Other if is_known_server_rejection(&error) => {
            PublishErrorKind::Rejected
        }
        NatsPublishErrorKind::TimedOut
        | NatsPublishErrorKind::BrokenPipe
        | NatsPublishErrorKind::Other => PublishErrorKind::ConfirmationUnknown,
    };
    PublishError::with_source(kind, error)
}

fn is_known_server_rejection(error: &NatsPublishError) -> bool {
    // Other also wraps decoding failures and ambiguous server failures. Only
    // a typed, known pre-persistence rejection changes the caller's policy.
    error
        .source()
        .and_then(|cause| cause.downcast_ref::<jetstream::Error>())
        .is_some_and(|cause| {
            cause.error_code() == jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM
        })
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
        ConsumerIntake, PreparedPublish, acknowledgement_kind, delivery_message_key,
        map_acknowledgement_error, map_send_error, prepare_publish, receipt,
        validate_consumer_configuration, validate_consumer_sequence,
        validate_settlement_confirmation,
    };
    use async_nats::StatusCode;
    use async_nats::jetstream::AckKind;
    use async_nats::jetstream::consumer::{AckPolicy, Config as ConsumerConfig};
    use async_nats::jetstream::context::{PublishError, PublishErrorKind as NatsPublishErrorKind};
    use async_nats::jetstream::publish::PublishAck;
    use edgeagent_contracts::{
        Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageMetadata,
        MessageRoutingError,
    };
    use edgeagent_messaging::{
        ConsumeError, ConsumeErrorKind, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey,
        DeliveryMetadata, DeliverySettlement, DeliverySubject, MessageDelivery, PublishDisposition,
        PublishErrorKind, RetryDelay, SettlementFuture,
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

    fn metadata() -> MessageMetadata {
        MessageMetadata {
            id: "message-01".to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: "order/order-01".to_owned(),
            time: "2026-09-26T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: "correlation-01".to_owned(),
            causation_id: "request-01".to_owned(),
            idempotency_key: "order-01".to_owned(),
            partition_key: "order/order-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        }
    }

    fn envelope() -> Result<edgeagent_contracts::MessageEnvelope, MessageRoutingError> {
        COMMAND.build(metadata(), &json!({"mode": "dry_run"}))
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
            COMMAND.message_type(),
            COMMAND.data_schema(),
            Component::ExecutionSimulator,
            COMMAND.partition_prefix(),
            edgeagent_contracts::EventRetention::Workflow,
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
        use NatsPublishErrorKind as Nats;
        use PublishErrorKind::{ConfirmationUnknown, Rejected, Unavailable};
        for (kind, send, acknowledgement) in [
            (Nats::StreamNotFound, Rejected, Rejected),
            (Nats::WrongLastMessageId, Rejected, Rejected),
            (Nats::WrongLastSequence, Rejected, Rejected),
            (Nats::MaxPayloadExceeded, Rejected, Rejected),
            (Nats::MaxAckPending, Unavailable, Rejected),
            (Nats::TimedOut, Unavailable, ConfirmationUnknown),
            (Nats::BrokenPipe, Unavailable, ConfirmationUnknown),
            (Nats::Other, Unavailable, ConfirmationUnknown),
        ] {
            assert_eq!(
                map_send_error(PublishError::new(kind)).kind(),
                send,
                "send {kind:?}"
            );
            assert_eq!(
                map_acknowledgement_error(PublishError::new(kind)).kind(),
                acknowledgement,
                "acknowledgement {kind:?}"
            );
        }
    }

    #[test]
    fn undecodable_and_unclassified_server_replies_remain_unknown() -> Result<(), Box<dyn Error>> {
        let decoding = serde_json::from_slice::<serde_json::Value>(b"not JSON")
            .err()
            .ok_or("fixture must fail decoding")?;
        let error = map_acknowledgement_error(PublishError::with_source(
            NatsPublishErrorKind::Other,
            decoding,
        ));
        assert_eq!(error.kind(), PublishErrorKind::ConfirmationUnknown);
        assert!(
            error
                .source()
                .and_then(|source| source.source())
                .is_some_and(|source| source.is::<serde_json::Error>())
        );

        for code in [10077, 19999] {
            let cause: async_nats::jetstream::Error = serde_json::from_value(json!({
                "code": 500, "err_code": code, "description": "unclassified server failure"
            }))?;
            let error = map_acknowledgement_error(PublishError::with_source(
                NatsPublishErrorKind::Other,
                cause,
            ));
            assert_eq!(error.kind(), PublishErrorKind::ConfirmationUnknown);
        }
        Ok(())
    }

    #[test]
    fn oversized_publication_is_rejected_by_contract_preparation() -> Result<(), Box<dyn Error>> {
        let envelope = edgeagent_contracts::MessageEnvelope::from_payload(
            metadata(),
            &json!({"padding": "x".repeat(MAX_PORTABLE_MESSAGE_BYTES)}),
        )?;
        assert!(matches!(
            prepare_publish(COMMAND, &envelope),
            Err(MessageRoutingError::EnvelopeTooLarge { .. })
        ));
        Ok(())
    }

    fn confirmation(status: Option<StatusCode>, payload: &[u8]) -> async_nats::Message {
        async_nats::Message {
            subject: "confirmation.inbox".into(),
            reply: None,
            payload: payload.to_vec().into(),
            headers: None,
            status,
            description: Some("private-confirmation-description-sentinel".to_owned()),
            length: payload.len(),
        }
    }

    #[test]
    fn only_successful_empty_responses_confirm_settlement() -> Result<(), Box<dyn Error>> {
        for status in [None, Some(StatusCode::OK)] {
            assert!(validate_settlement_confirmation(&confirmation(status, b"")).is_ok());
        }
        for status in [
            StatusCode::NO_RESPONDERS,
            StatusCode::REQUEST_TERMINATED,
            StatusCode::IDLE_HEARTBEAT,
        ] {
            let result = validate_settlement_confirmation(&confirmation(Some(status), b""));
            assert_eq!(
                result.err().map(|error| error.kind()),
                Some(ConsumeErrorKind::ConfirmationUnknown)
            );
        }
        for status in [None, Some(StatusCode::OK)] {
            let result = validate_settlement_confirmation(&confirmation(
                status,
                b"private-confirmation-body-sentinel",
            ));
            let error = result.err().ok_or("nonempty confirmation must fail")?;
            assert_eq!(error.kind(), ConsumeErrorKind::ConfirmationUnknown);
            assert!(!format!("{error} {error:?}").contains("private-confirmation"));
            let source = error.source().map(ToString::to_string).unwrap_or_default();
            assert!(!source.contains("private-confirmation"));
            assert!(source.contains("payload bytes"));
        }
        Ok(())
    }

    #[test]
    fn typed_server_size_rejection_is_not_an_unknown_confirmation() -> Result<(), Box<dyn Error>> {
        let cause: async_nats::jetstream::Error = serde_json::from_value(json!({
            "code": 400,
            "err_code": 10054,
            "description": "private-server-rejection-sentinel"
        }))?;
        let error = map_acknowledgement_error(PublishError::with_source(
            NatsPublishErrorKind::Other,
            cause,
        ));
        assert_eq!(error.kind(), PublishErrorKind::Rejected);
        assert!(
            !error
                .to_string()
                .contains("private-server-rejection-sentinel")
        );
        assert!(!format!("{error:?}").contains("private-server-rejection-sentinel"));
        let sdk = error
            .source()
            .and_then(|cause| cause.downcast_ref::<PublishError>())
            .ok_or("SDK cause must remain available")?;
        assert!(
            sdk.source()
                .is_some_and(|cause| cause.is::<async_nats::jetstream::Error>())
        );
        Ok(())
    }

    #[test]
    fn header_only_consumer_is_rejected_before_intake() {
        let config = ConsumerConfig {
            durable_name: Some("headers_only_v1".to_owned()),
            ack_policy: AckPolicy::Explicit,
            headers_only: true,
            ..ConsumerConfig::default()
        };
        assert_eq!(
            validate_consumer_configuration(&config)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
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
    fn settlement_maps_to_confirmed_acknowledgement_kinds() -> Result<(), Box<dyn Error>> {
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::Acknowledge),
            AckKind::Ack
        ));
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::Quarantined),
            AckKind::Term
        ));
        let delay = RetryDelay::new(Duration::from_secs(2))?;
        assert!(matches!(
            acknowledgement_kind(DeliveryDisposition::RetryAfter(delay)),
            AckKind::Nak(Some(delay)) if delay == Duration::from_secs(2)
        ));
        Ok(())
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
        assert_eq!(
            delivery_message_key("ORDERS", 0)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
    }

    #[test]
    fn broker_consumer_sequence_is_validated_without_leaking_into_portable_metadata() {
        assert_eq!(
            validate_consumer_sequence(0)
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert!(validate_consumer_sequence(1).is_ok());
    }

    #[test]
    fn malformed_metadata_halts_this_consumer_without_retrying_the_pull() {
        let (mut gate, drops) = intake_probe();
        assert!(gate.stream().is_ok());
        assert!(matches!(gate.validate(Ok(41_u64)), Ok(41)));
        assert!(gate.stream().is_ok());

        let error = ConsumeError::protocol("invalid broker metadata");
        assert_eq!(
            gate.validate::<u64>(Err(error))
                .err()
                .map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            gate.stream().err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
        drop(gate);
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    struct IntakeProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for IntakeProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    impl futures_util::Stream for IntakeProbe {
        type Item = ();

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Pending
        }
    }

    fn intake_probe() -> (
        ConsumerIntake<IntakeProbe>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (ConsumerIntake::Active(IntakeProbe(drops.clone())), drops)
    }

    #[test]
    fn terminal_pull_results_release_the_stream_and_repeated_receive_stays_closed()
    -> Result<(), Box<dyn Error>> {
        use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
        for result in [
            None,
            Some(Err(MessagesError::new(MessagesErrorKind::ConsumerDeleted))),
            Some(Err(MessagesError::new(
                MessagesErrorKind::PushBasedConsumer,
            ))),
        ] {
            let expected = result
                .as_ref()
                .and_then(|result: &Result<(), MessagesError>| {
                    result.as_ref().err().map(MessagesError::kind)
                });
            let (mut intake, drops) = intake_probe();
            let error = intake.resolve_pull(result).err().ok_or("terminal error")?;
            assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
            assert_eq!(
                error
                    .source()
                    .and_then(|source| source.downcast_ref::<MessagesError>())
                    .map(MessagesError::kind),
                expected
            );
            for _ in 0..2 {
                assert_eq!(
                    intake.stream().err().map(|error| error.kind()),
                    Some(ConsumeErrorKind::Unavailable)
                );
            }
            assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
            drop(intake);
            assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
        }
        Ok(())
    }

    #[test]
    fn transient_pull_errors_preserve_stream_and_typed_cause() -> Result<(), Box<dyn Error>> {
        use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
        for kind in [
            MessagesErrorKind::MissingHeartbeat,
            MessagesErrorKind::Pull,
            MessagesErrorKind::NoResponders,
            MessagesErrorKind::Other,
        ] {
            let (mut intake, drops) = intake_probe();
            let error = intake
                .resolve_pull::<()>(Some(Err(MessagesError::with_source(
                    kind,
                    std::io::Error::other("private-pull-sentinel"),
                ))))
                .err()
                .ok_or("transient error")?;
            assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
            let cause = error
                .source()
                .and_then(|source| source.downcast_ref::<MessagesError>())
                .ok_or("SDK cause")?;
            assert_eq!(cause.kind(), kind);
            assert!(
                cause
                    .source()
                    .is_some_and(|source| source.is::<std::io::Error>())
            );
            assert!(!error.to_string().contains("private-pull-sentinel"));
            assert!(!format!("{error:?}").contains("private-pull-sentinel"));
            assert!(intake.stream().is_ok());
            assert_eq!(intake.resolve_pull(Some(Ok(42)))?, 42);
            assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
        }
        Ok(())
    }

    #[test]
    fn cancelling_a_pending_stream_borrow_keeps_intake_active() -> Result<(), Box<dyn Error>> {
        use futures_util::{FutureExt, StreamExt};
        let (mut intake, drops) = intake_probe();
        assert!(intake.stream()?.next().now_or_never().is_none());
        assert!(intake.stream().is_ok());
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
        drop(intake);
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
        Ok(())
    }

    struct NoopSettlement;

    impl DeliverySettlement for NoopSettlement {
        fn settle(self: Box<Self>, _disposition: DeliveryDisposition) -> SettlementFuture {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn oversized_payload_halts_this_consumer_before_another_pull() -> Result<(), Box<dyn Error>> {
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("6:STREAM:1")?,
            DeliverySubject::new(COMMAND.subject()?)?,
            DeliveryAttempt::new(1)?,
        );
        let (mut gate, drops) = intake_probe();

        let result = gate.validate(MessageDelivery::new(
            vec![0; MAX_PORTABLE_MESSAGE_BYTES + 1],
            metadata,
            Box::new(NoopSettlement),
        ));

        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(
            gate.stream().err().map(|error| error.kind()),
            Some(ConsumeErrorKind::Protocol)
        );
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
        Ok(())
    }
}
