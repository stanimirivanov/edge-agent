//! Application-owned messaging ports and portable delivery outcomes.
//!
//! Publication, consumption, atomic inbound processing, and outbox relay
//! storage remain separate capability contracts behind this stable façade.
//!
//! # Ownership and canonical flows
//!
//! | Capability | Port | Responsibility |
//! | --- | --- | --- |
//! | Intake | [`MessageConsumer`] | Return one bounded, unsettled delivery |
//! | Inbound persistence | [`InboundMessageStore`] | Atomically deduplicate/domain-commit or retain exact quarantine evidence |
//! | Broker settlement | [`DeliverySettlement`] | Confirm the one disposition chosen by application policy |
//! | Outbound persistence | [`OutboxRelayStore`] | Commit a claim and fence its completion |
//! | Publication | [`MessagePublisher`] | Validate routing and await durable transport confirmation |
//!
//! The inbound coordinator (`edgeagent-inbox-handler`) owns these orderings:
//!
//! ```text
//! receive -> decode/validate -> process -> Acknowledge (Applied or Duplicate)
//! receive -> classify terminal failure -> quarantine -> Quarantined
//! confirmed transient/ambiguous error -> bounded policy -> RetryAfter
//! ```
//!
//! Persistence adapters own transactions; settlement never substitutes for
//! committed inbox/domain work or retained quarantine evidence. Malformed bytes
//! within the delivery bound can be retained without a decoded envelope.
//! Cancellation is not a classified handler error and chooses no disposition.
//!
//! The outbound coordinator (`edgeagent-outbox-relay`) owns:
//!
//! ```text
//! committed claim -> revalidate -> publish outside transaction -> fenced outcome
//! ```
//!
//! A receipt permits `mark_published`; classified failures select bounded retry
//! or quarantine. Expiry/reacquisition advances the claim generation. A stale
//! worker cannot complete the newer claim, but fencing cannot undo publication.
//! Broker deduplication is bounded; durable consumer inbox identity remains
//! necessary for at-least-once delivery to produce one committed domain effect.
//!
//! # Futures and cancellation
//!
//! Boxed `Send` futures keep the ports dyn-compatible, allowing runtime adapter
//! composition without broker or database types in application policy. Their
//! lifetimes express borrowed operations; [`SettlementFuture`] owns its one-shot
//! confirmation and is `'static`. No additional `Sync` bound is required for a
//! delivery: the coordinator owns it while borrowing its evidence.
//!
//! Unless an adapter documents a stronger guarantee, constructing a port future
//! may perform synchronous work. Dropping an unpolled or pending future therefore
//! proves neither that an effect was absent nor that a transaction rolled back.
//! Await explicit results; resolve unknown outcomes through immutable retry
//! identity, durable state, and current claim fences. `Drop` never starts a
//! replacement operation. Per-method cancellation sections define recovery.
//!
//! # Commit before terminal settlement
//!
//! This in-memory persistence future and settlement stub exercise the ordering
//! without a runtime. They are not a durable adapter or a production handler.
//!
//! ```
//! use edgeagent_messaging::{
//!     DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata,
//!     DeliverySettlement, DeliverySubject, InboxFuture, InboxStoreError,
//!     MessageDelivery, QuarantineDisposition, SettlementFuture,
//! };
//! use std::future::Future;
//! use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
//! use std::task::{Context, Poll, Waker};
//!
//! struct ConfirmAfterCommit(Arc<AtomicBool>);
//! impl DeliverySettlement for ConfirmAfterCommit {
//!     fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
//!         assert!(self.0.load(Ordering::SeqCst));
//!         assert_eq!(disposition, DeliveryDisposition::Quarantined);
//!         Box::pin(std::future::ready(Ok(())))
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let committed = Arc::new(AtomicBool::new(false));
//! let store_commit = Arc::clone(&committed);
//! let persistence: InboxFuture<'_, Result<QuarantineDisposition, InboxStoreError>> =
//!     Box::pin(async move {
//!         store_commit.store(true, Ordering::SeqCst);
//!         Ok(QuarantineDisposition::Inserted)
//!     });
//! let delivery = MessageDelivery::new(
//!     b"{malformed".to_vec(),
//!     DeliveryMetadata::new(
//!         DeliveryMessageKey::new("commands:41")?,
//!         DeliverySubject::new("edgeagent.command.execution.submit-dry-run-order.v1")?,
//!         DeliveryAttempt::new(1)?,
//!     ),
//!     Box::new(ConfirmAfterCommit(committed)),
//! )?;
//! let mut handling = Box::pin(async move {
//!     persistence.await?;
//!     delivery.settle(DeliveryDisposition::Quarantined).await?;
//!     Ok::<(), Box<dyn std::error::Error>>(())
//! });
//! assert!(matches!(
//!     handling.as_mut().poll(&mut Context::from_waker(Waker::noop())),
//!     Poll::Ready(Ok(()))
//! ));
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod consumer;
mod failure_code;
mod inbox;
mod metadata;
mod outbox;
mod publisher;

pub use consumer::{
    ConsumeError, ConsumeErrorKind, DeliveryDisposition, DeliverySettlement, MessageConsumer,
    MessageDelivery, ReceiveFuture, RetryDelay, SettlementFuture,
};
pub use failure_code::FailureCode;
pub use inbox::{
    HandlerFailure, HandlerFailureKind, InboundMessageStore, InboundProcessingError,
    InboundQuarantine, InboxDisposition, InboxFuture, InboxStoreError, InboxStoreErrorKind,
    QuarantineDisposition,
};
pub use metadata::{DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata, DeliverySubject};
pub use outbox::{
    ClaimedMessage, LeaseDuration, LeaseGeneration, OutboxRelayStore, OutboxRetryDelay,
    OutboxStoreError, OutboxStoreErrorKind, OutboxStoreFuture, OutboxTimingError,
};
pub use publisher::{
    MessagePublisher, PublishDisposition, PublishError, PublishErrorKind, PublishFuture,
    PublishReceipt,
};
