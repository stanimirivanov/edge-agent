//! Intake state owns admission to pulls, never broker settlement.

use async_nats::jetstream::consumer::pull;
use edgeagent_messaging::{ConsumeError, ConsumeErrorKind};
use futures_util::{Stream, StreamExt};

#[cfg(test)]
mod tests;

// Terminal states cannot retain intake resources. A polled receive owns its
// worker separately, so a paused caller cannot retain an expiry/reconnect task.
pub(super) enum ConsumerIntake<S> {
    Active(S),
    Halted,
    Closed,
}

impl<S> ConsumerIntake<S> {
    pub(super) fn active(&mut self) -> Result<&mut S, ConsumeError> {
        match self {
            Self::Active(stream) => Ok(stream),
            Self::Halted => Err(ConsumeError::protocol(
                "consumer halted after invalid broker delivery",
            )),
            Self::Closed => Err(ConsumeError::unavailable("consumer stream ended")),
        }
    }

    pub(super) fn resolve_pull<T>(
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

    pub(super) fn validate<T>(
        &mut self,
        result: Result<T, ConsumeError>,
    ) -> Result<T, ConsumeError> {
        if result.is_err() {
            *self = Self::Halted;
        }
        result
    }
}

impl<S> std::fmt::Debug for ConsumerIntake<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Active(_) => "Active",
            Self::Halted => "Halted",
            Self::Closed => "Closed",
        })
    }
}

// Ownership stays inside this future. Completion, failure, or cancellation
// drops the SDK stream and aborts its renewal task without a broker disposition.
pub(super) async fn receive_from<S: Stream + Unpin>(mut stream: S) -> Option<S::Item> {
    stream.next().await
}
