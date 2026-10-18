//! Synthetic parent/child barriers at actual durable boundaries, never adapter hooks.

use std::io::Write;

use edgeagent_contracts::{MessageDefinition, MessageEnvelope};
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliverySettlement, MessageDelivery,
    MessagePublisher, PublishError, PublishErrorKind, PublishFuture, SettlementFuture,
};
use edgeagent_messaging_nats::JetStreamPublisher;
use tokio::io::AsyncReadExt;

use super::{Role, WorkerError};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Boundary {
    BeforeCommit,
    Enqueued,
    Published,
    Committed,
}

impl Boundary {
    pub(super) fn parse(role: Role, action: u8) -> Result<Option<Self>, WorkerError> {
        match (role, action) {
            (Role::Producer, b'b') => Ok(Some(Self::BeforeCommit)),
            (Role::Producer, b'e') => Ok(Some(Self::Enqueued)),
            (Role::Relay, b'u') => Ok(Some(Self::Published)),
            (Role::Consumer, b'a') => Ok(Some(Self::Committed)),
            (_, b'b' | b'e' | b'u' | b'a') => Err("fault action does not match role".into()),
            _ => Ok(None),
        }
    }
}

pub(super) async fn barrier(label: &str, input: &mut tokio::io::Stdin) -> Result<(), WorkerError> {
    println!("{label}");
    std::io::stdout().flush()?;
    let mut release = [0];
    input.read_exact(&mut release).await?;
    if release != *b"c" {
        return Err("invalid fault barrier release".into());
    }
    Ok(())
}

pub(super) struct Publisher {
    pub(super) inner: JetStreamPublisher,
    pub(super) hold: bool,
}

impl MessagePublisher for Publisher {
    fn publish<'a>(
        &'a self,
        definition: MessageDefinition,
        envelope: &'a MessageEnvelope,
    ) -> PublishFuture<'a> {
        Box::pin(async move {
            let receipt = self.inner.publish(definition, envelope).await?;
            if self.hold {
                // Real persistence is confirmed, but relay completion has not run.
                barrier("persisted", &mut tokio::io::stdin())
                    .await
                    .map_err(|_| PublishError::new(PublishErrorKind::ConfirmationUnknown))?;
            }
            Ok(receipt)
        })
    }
}

struct Settlement(MessageDelivery);

impl DeliverySettlement for Settlement {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async move {
            // The real coordinator invokes settlement only after the store's
            // transaction returns. The parent independently inspects that commit.
            barrier("committed", &mut tokio::io::stdin())
                .await
                .map_err(|_| ConsumeError::new(ConsumeErrorKind::ConfirmationUnknown))?;
            self.0.settle(disposition).await
        })
    }
}

pub(super) fn hold_settlement(delivery: MessageDelivery) -> Result<MessageDelivery, ConsumeError> {
    MessageDelivery::new(
        delivery.payload().to_vec(),
        delivery.metadata().clone(),
        Box::new(Settlement(delivery)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_actions_are_closed_and_role_scoped() -> Result<(), WorkerError> {
        for (role, action) in [
            (Role::Producer, b'b'),
            (Role::Producer, b'e'),
            (Role::Relay, b'u'),
            (Role::Consumer, b'a'),
        ] {
            assert!(Boundary::parse(role, action)?.is_some());
            assert!(Boundary::parse(Role::ReplayAuthorize, action).is_err());
        }
        assert!(Boundary::parse(Role::Producer, b'c')?.is_none());
        Ok(())
    }
}
