//! Application-owned messaging ports and portable delivery outcomes.
//!
//! Publication, consumption, atomic inbound processing, and outbox relay
//! storage remain separate capability contracts behind this stable façade.

#![forbid(unsafe_code)]

mod consumer;
mod inbox;
mod metadata;
mod outbox;
mod publisher;

pub use consumer::{
    ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliverySettlement, MessageConsumer,
    MessageDelivery, ReceiveFuture, RetryDelay, SettlementFuture,
};
pub use inbox::{
    HandlerFailure, HandlerFailureKind, InboundMessageStore, InboundProcessingError,
    InboundQuarantine, InboxDisposition, InboxFuture, InboxStoreError, InboxStoreErrorKind,
    QuarantineDisposition,
};
pub use metadata::{DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata, DeliverySubject};
pub use outbox::{
    ClaimedMessage, LeaseGeneration, OutboxRelayStore, OutboxStoreError, OutboxStoreErrorKind,
    OutboxStoreFuture,
};
pub use publisher::{
    MessagePublisher, PublishDisposition, PublishError, PublishErrorKind, PublishFuture,
    PublishReceipt,
};
