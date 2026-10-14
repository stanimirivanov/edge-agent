//! Publication preparation and phase-aware broker confirmation.

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
use std::error::Error;

#[cfg(test)]
mod tests;

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
