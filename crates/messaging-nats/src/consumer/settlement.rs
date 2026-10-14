//! One-shot settlement and payload-safe confirmation validation.

use async_nats::jetstream::AckKind;
use async_nats::{Request, StatusCode};
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliverySettlement, SettlementFuture,
};
use std::error::Error;
use std::time::Duration;

#[cfg(test)]
mod tests;

pub(super) struct JetStreamSettlement {
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

impl JetStreamSettlement {
    pub(super) fn new(
        client: async_nats::Client,
        reply: async_nats::Subject,
        timeout: Duration,
    ) -> Self {
        Self {
            client,
            reply,
            timeout,
        }
    }
}
