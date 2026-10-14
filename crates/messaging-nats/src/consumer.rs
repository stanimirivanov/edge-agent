//! Receive-scoped pull ownership, validation, and portable delivery handoff.

mod intake;
mod metadata;
mod settlement;

#[cfg(test)]
mod tests;

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, PullConsumer, pull};
use async_nats::jetstream::context::traits::TimeoutProvider;
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, MessageConsumer, MessageDelivery, ReceiveFuture,
};
use intake::{ConsumerIntake, receive_from};
use settlement::JetStreamSettlement;

/// Pull-based JetStream consumer behind the portable one-delivery port.
///
/// The supplied consumer must be durable, deliver complete message bodies, and
/// use explicit acknowledgements. Provision application-compatible subject, retention, maximum
/// delivery, acknowledgement wait, and pending limits. Provisioning remains a
/// deployment concern. Each request asks for one message; no pull worker remains
/// after receive completes or is cancelled. This is not a broker-global pending limit.
/// Invalid broker metadata or a payload outside portable bounds leaves the raw
/// message unsettled and halts this instance before another pull; replacement
/// requires operator investigation.
/// Each polled receive owns a pull stream and releases it on completion or cancellation.
/// Already-issued requests are not revoked. Stream closure and terminal SDK
/// errors close intake; transient pull errors preserve it. Cancelling a pending
/// receive leaves intake active and does not acknowledge any delivery.
pub struct JetStreamConsumer {
    intake: ConsumerIntake<ActiveIntake>,
}

struct ActiveIntake {
    configured: PullConsumer,
    initial_stream: Option<pull::Stream>,
}

impl JetStreamConsumer {
    /// Validate a configured durable consumer and prepare its first pull stream.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` for incompatible cached configuration, or `Unavailable`
    /// if stream setup fails. Quiesce and recreate after provisioning changes.
    pub async fn new(consumer: PullConsumer) -> Result<Self, ConsumeError> {
        validate_consumer_configuration(&consumer.cached_info().config)?;
        let initial_stream = start_stream(&consumer).await?;
        Ok(Self {
            intake: ConsumerIntake::Active(ActiveIntake {
                configured: consumer,
                initial_stream: Some(initial_stream),
            }),
        })
    }
}

impl std::fmt::Debug for JetStreamConsumer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetStreamConsumer")
            .field("intake", &self.intake)
            .finish_non_exhaustive()
    }
}

async fn start_stream(consumer: &PullConsumer) -> Result<pull::Stream, ConsumeError> {
    consumer
        .stream()
        .max_messages_per_batch(1)
        .messages()
        .await
        .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Unavailable, error))
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
            let active = self.intake.active()?;
            // The optional initial stream preserves constructor admission without
            // pulling before receive. Later receives create fresh, bounded streams.
            let stream = match active.initial_stream.take() {
                Some(stream) => stream,
                None => start_stream(&active.configured).await?,
            };
            let result = receive_from(stream).await;
            // The receive-owned worker is gone BEFORE validation and handoff.
            // There is no await after receipt, hence no cancellation gap in this
            // sequence. Already-issued requests still have an unknown outcome.
            let message = self.intake.resolve_pull(result)?;
            let metadata = self.intake.validate(metadata::derive(&message))?;
            let jetstream::Message { message, context } = message;
            let reply = self.intake.validate(message.reply.ok_or_else(|| {
                ConsumeError::protocol("delivery must contain a settlement reply subject")
            }))?;
            self.intake.validate(MessageDelivery::new(
                message.payload,
                metadata,
                Box::new(JetStreamSettlement::new(
                    context.client(),
                    reply,
                    context.timeout(),
                )),
            ))
        })
    }
}
