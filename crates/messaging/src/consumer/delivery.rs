//! One-shot delivery ownership and payload-safe abandonment diagnostics.

use super::{ConsumeError, DeliveryDisposition, DeliverySettlement, SettlementFuture};
use crate::metadata::DeliveryMetadata;
use bytes::Bytes;
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use std::fmt::Formatter;

/// A size-bounded, otherwise untrusted delivery owning one broker settlement.
///
/// Dropping it or its incomplete settlement future emits one payload-safe
/// warning. A warning is not a durable disposition and never settles a message.
#[must_use = "a delivery must be handled and explicitly settled"]
pub struct MessageDelivery {
    payload: Bytes,
    metadata: DeliveryMetadata,
    settlement: Box<dyn DeliverySettlement>,
    warning: AbandonmentWarning,
}

impl MessageDelivery {
    /// Construct a delivery from a transport adapter without copying a `Bytes`
    /// payload. An owned byte vector is also accepted without copying its bytes.
    ///
    /// # Errors
    ///
    /// Returns `Protocol` if raw bytes exceed the portable 256 KiB envelope
    /// limit. The adapter must leave that raw broker message unsettled and
    /// follow its protocol-fault recovery policy.
    pub fn new(
        payload: impl Into<Bytes>,
        metadata: DeliveryMetadata,
        settlement: Box<dyn DeliverySettlement>,
    ) -> Result<Self, ConsumeError> {
        let payload = payload.into();
        if payload.len() > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(ConsumeError::protocol(
                "delivery payload exceeds portable 256 KiB limit",
            ));
        }
        let warning = AbandonmentWarning {
            delivery_attempt: metadata.delivery_attempt(),
            payload_bytes: payload.len(),
            state: OwnershipState::Delivery,
        };
        Ok(Self {
            payload,
            metadata,
            settlement,
            warning,
        })
    }

    /// Return the untrusted structured envelope bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Return portable broker delivery metadata.
    #[must_use]
    pub const fn metadata(&self) -> &DeliveryMetadata {
        &self.metadata
    }

    /// Consume this delivery and await its terminal broker disposition.
    ///
    /// The adapter method is called synchronously here; its returned future
    /// must be awaited to obtain a confirmation result. Dropping that future
    /// before completion warns once but cannot establish whether a request
    /// reached the broker, even if it was never polled. It performs no automatic
    /// acknowledgement, retry, or quarantine. Handlers must deduplicate by
    /// stable message identity if redelivered after committing local work.
    ///
    /// The wrapper releases this delivery's payload and metadata immediately
    /// and retains only scalar diagnostic fields. Explicit success or error
    /// results pass through unchanged and disarm the abandonment warning.
    ///
    /// A delivery cannot choose a second disposition:
    ///
    /// ```compile_fail,E0382
    /// use edgeagent_messaging::{DeliveryDisposition, MessageDelivery};
    ///
    /// fn settle_twice(delivery: MessageDelivery) {
    ///     let first = delivery.settle(DeliveryDisposition::Acknowledge);
    ///     let second = delivery.settle(DeliveryDisposition::Quarantined);
    ///     drop((first, second));
    /// }
    /// ```
    ///
    /// Ignoring the confirmation future is a caller error:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use edgeagent_messaging::{DeliveryDisposition, MessageDelivery};
    ///
    /// fn abandon_confirmation(delivery: MessageDelivery) {
    ///     delivery.settle(DeliveryDisposition::Acknowledge);
    /// }
    /// ```
    #[must_use = "settlement must be awaited and its confirmation result inspected"]
    pub fn settle(self, disposition: DeliveryDisposition) -> SettlementFuture {
        let Self {
            payload,
            metadata,
            settlement,
            mut warning,
        } = self;
        // The future needs neither raw bytes nor identities for its diagnostic.
        drop((payload, metadata));
        warning.begin_settlement();
        // Keep the guard armed across adapter future construction as well as
        // polling; a synchronous adapter failure must not hide abandonment.
        let future = settlement.settle(disposition);
        Box::pin(async move {
            let result = future.await;
            // A returned error is explicit caller-visible failure, not a drop.
            warning.finish();
            result
        })
    }
}

impl std::fmt::Debug for MessageDelivery {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MessageDelivery")
            .field("payload_bytes", &self.payload.len())
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

enum OwnershipState {
    Delivery,
    Settlement,
    Finished,
}

// One guard has one owner throughout the lifecycle. No payload, identity,
// adapter error, or broker recovery policy belongs in this best-effort warning.
struct AbandonmentWarning {
    delivery_attempt: u32,
    payload_bytes: usize,
    state: OwnershipState,
}

impl AbandonmentWarning {
    fn begin_settlement(&mut self) {
        self.state = OwnershipState::Settlement;
    }

    fn finish(&mut self) {
        self.state = OwnershipState::Finished;
    }
}

impl Drop for AbandonmentWarning {
    fn drop(&mut self) {
        match self.state {
            OwnershipState::Delivery => tracing::warn!(
                delivery_attempt = self.delivery_attempt,
                payload_bytes = self.payload_bytes,
                "message delivery dropped without settlement"
            ),
            OwnershipState::Settlement => tracing::warn!(
                delivery_attempt = self.delivery_attempt,
                payload_bytes = self.payload_bytes,
                settlement_state = "confirmation_unknown",
                "message settlement dropped without completion"
            ),
            OwnershipState::Finished => {}
        }
    }
}
