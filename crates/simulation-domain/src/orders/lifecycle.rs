use std::collections::BTreeMap;

use edgeagent_market_data_domain::PreciseInstant;

use super::{OrderDefinition, OrderError, OrderEvent, OrderEventId, OrderEventKind, OrderQuantity};

/// Lifecycle projection reconstructed solely from ordered retained facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderStatus {
    /// Submission exists, but admission has not succeeded.
    Submitted,
    /// Accepted, with no fills yet.
    Working,
    /// Accepted, with some quantity filled.
    PartiallyFilled,
    /// Cancellation is unresolved; fills may still arrive.
    CancelPending,
    /// All original quantity was filled.
    Filled,
    /// Unfilled remainder was definitively cancelled.
    Cancelled,
    /// Execution deadline passed; historical fills remain.
    Expired,
    /// Admission rejected before any execution.
    Rejected,
}
impl OrderStatus {
    pub(super) const fn fillable(self) -> bool {
        matches!(
            self,
            Self::Working | Self::PartiallyFilled | Self::CancelPending
        )
    }
    const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Rejected
        )
    }
}

/// Whether a retained identity caused a new transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApplyOutcome {
    /// New fact applied once.
    Applied,
    /// Exact retained content; state did not change.
    Duplicate,
}

/// Bounded replayable order projection. No transport, admission or ledger authority.
/// Reconstruct from the original submission and complete journal, never an empty
/// aggregate bearing an existing order ID. P08 supplies durable atomic ownership.
pub struct Order {
    pub(super) definition: OrderDefinition,
    pub(super) status: OrderStatus,
    filled: u64,
    revision: u64,
    last_time: PreciseInstant,
    capacity: usize,
    events: Vec<OrderEvent>,
    identities: BTreeMap<OrderEventId, usize>,
}
impl std::fmt::Debug for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Order")
            .field("scope", &self.definition.scope)
            .field("status", &self.status)
            .field("revision", &self.revision)
            .field("retained_events", &self.events.len())
            .finish_non_exhaustive()
    }
}
impl Order {
    /// Start an unadmitted projection with a finite, non-evicting journal.
    ///
    /// # Errors
    /// Returns `Capacity` unless the event bound is in `1..=4096`.
    pub fn new(definition: OrderDefinition, event_capacity: usize) -> Result<Self, OrderError> {
        if !(1..=4096).contains(&event_capacity) {
            return Err(OrderError::Capacity);
        }
        let last_time = definition.lifetime.bounds().0;
        Ok(Self {
            definition,
            status: OrderStatus::Submitted,
            filled: 0,
            revision: 0,
            last_time,
            capacity: event_capacity,
            events: Vec::new(),
            identities: BTreeMap::new(),
        })
    }
    /// Compare the entire immutable submission, not only its order ID.
    ///
    /// # Errors
    /// Returns `IdentityConflict` if any immutable field differs.
    pub fn retry_submission(
        &self,
        definition: &OrderDefinition,
    ) -> Result<ApplyOutcome, OrderError> {
        if &self.definition == definition {
            Ok(ApplyOutcome::Duplicate)
        } else {
            Err(OrderError::IdentityConflict)
        }
    }
    /// Immutable submission and pinned product terms.
    #[must_use]
    pub const fn definition(&self) -> &OrderDefinition {
        &self.definition
    }
    /// Current lifecycle status and last committed-to-this-projection revision.
    #[must_use]
    pub const fn state(&self) -> (OrderStatus, u64) {
        (self.status, self.revision)
    }
    /// Filled and unfilled whole quantities; zero is represented by `None`.
    #[must_use]
    pub fn quantities(&self) -> (Option<OrderQuantity>, Option<OrderQuantity>) {
        let original = self.definition.quantity;
        (
            OrderQuantity::with_unit(original.unit, self.filled).ok(),
            OrderQuantity::with_unit(original.unit, original.count - self.filled).ok(),
        )
    }
    /// Complete ordered history, including non-fill decisions and cancel attempts.
    #[must_use]
    pub fn events(&self) -> &[OrderEvent] {
        &self.events
    }
    pub(super) fn retained(&self, id: OrderEventId) -> Option<&OrderEvent> {
        self.identities
            .get(&id)
            .and_then(|index| self.events.get(*index))
    }
    /// Apply a fact only after validating all invariants. Every error leaves the
    /// projection unchanged; exact duplicates remain valid after terminal states
    /// and journal exhaustion. Gaps require recovery, not speculative application.
    ///
    /// # Errors
    /// Returns identity, revision, time, capacity, lifecycle, unit, price or quantity
    /// failures. Canonical revision order resolves fill/cancel races explicitly.
    pub fn apply(&mut self, event: OrderEvent) -> Result<ApplyOutcome, OrderError> {
        if event.order != self.definition.scope.order {
            return Err(OrderError::WrongOrder);
        }
        if let Some(retained) = self.retained(event.id) {
            return if retained == &event {
                Ok(ApplyOutcome::Duplicate)
            } else {
                Err(OrderError::IdentityConflict)
            };
        }
        let (status, filled) = self.project(event)?;
        self.identities.insert(event.id, self.events.len());
        self.events.push(event);
        self.status = status;
        self.filled = filled;
        self.revision = event.revision;
        self.last_time = event.at;
        Ok(ApplyOutcome::Applied)
    }
    pub(super) fn check_position(
        &self,
        revision: u64,
        at: PreciseInstant,
    ) -> Result<(), OrderError> {
        let next = self.revision.checked_add(1).ok_or(OrderError::Revision)?;
        if revision > next {
            return Err(OrderError::RevisionGap);
        }
        if revision != next {
            return Err(OrderError::Revision);
        }
        if at < self.last_time {
            return Err(OrderError::Time);
        }
        if self.events.len() == self.capacity {
            return Err(OrderError::Capacity);
        }
        if self.status.terminal() {
            return Err(OrderError::State);
        }
        Ok(())
    }
    pub(super) fn check_execution(
        &self,
        revision: u64,
        at: PreciseInstant,
    ) -> Result<(), OrderError> {
        self.check_position(revision, at)?;
        if at >= self.definition.lifetime.bounds().1 {
            return Err(OrderError::Time);
        }
        if !self.status.fillable() {
            return Err(OrderError::State);
        }
        Ok(())
    }
    pub(super) fn project(&self, event: OrderEvent) -> Result<(OrderStatus, u64), OrderError> {
        self.check_position(event.revision, event.at)?;
        if event.at >= self.definition.lifetime.bounds().1 {
            return if event.kind == OrderEventKind::Expired {
                Ok((OrderStatus::Expired, self.filled))
            } else {
                Err(OrderError::Time)
            };
        }
        let status = match event.kind {
            OrderEventKind::Accepted if self.status == OrderStatus::Submitted => {
                OrderStatus::Working
            }
            OrderEventKind::Rejected(_) if self.status == OrderStatus::Submitted => {
                OrderStatus::Rejected
            }
            OrderEventKind::CancelRequested
                if matches!(
                    self.status,
                    OrderStatus::Working | OrderStatus::PartiallyFilled
                ) =>
            {
                OrderStatus::CancelPending
            }
            OrderEventKind::CancelRejected if self.status == OrderStatus::CancelPending => {
                if self.filled == 0 {
                    OrderStatus::Working
                } else {
                    OrderStatus::PartiallyFilled
                }
            }
            OrderEventKind::Cancelled if self.status == OrderStatus::CancelPending => {
                OrderStatus::Cancelled
            }
            OrderEventKind::Filled(fill) if self.status.fillable() => {
                let proposal = fill.proposal;
                if proposal.quantity.unit != self.definition.quantity.unit {
                    return Err(OrderError::Unit);
                }
                self.definition.fill_price(proposal.price)?;
                let filled = self
                    .filled
                    .checked_add(proposal.quantity.count)
                    .ok_or(OrderError::Quantity)?;
                if filled > self.definition.quantity.count {
                    return Err(OrderError::Quantity);
                }
                let status = if filled == self.definition.quantity.count {
                    OrderStatus::Filled
                } else if self.status == OrderStatus::CancelPending {
                    OrderStatus::CancelPending
                } else {
                    OrderStatus::PartiallyFilled
                };
                return Ok((status, filled));
            }
            OrderEventKind::NoFill { .. } if self.status.fillable() => self.status,
            OrderEventKind::Expired => return Err(OrderError::Time),
            _ => return Err(OrderError::State),
        };
        Ok((status, self.filled))
    }
}
