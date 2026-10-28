//! Offline scheduling only: no I/O, durable lease, semantic qualification or readiness grant.
use crate::Cancellation;
use edgeagent_market_data_domain::{CanonicalObservation, FeedEpoch, FeedId, ProviderCapability};
use std::{collections::VecDeque, num::NonZeroU16, sync::Arc};

/// Sticky local incident. Clearing it requires later durable recovery composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StreamStop {
    /// A source or aggregate queue ceiling rejected intake; no head was evicted.
    Overload,
    /// The source reported a protocol, reconciliation, authority or processing failure.
    SourceFailure,
    /// Cooperative cancellation; does not establish external quiescence.
    Cancelled,
    /// A record or epoch contradicted the pinned lane binding.
    Binding,
}

/// Payload-free scheduling rejection, not a market-data acceptance decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SupervisionError {
    /// Require 1..=32 distinct feeds and positive queue bounds no greater than 4096.
    Configuration,
    /// No registered feed matches the request.
    UnknownSource,
    /// Dispatch belongs to another supervisor or no longer owns its exact head.
    ForeignDispatch,
    /// At least one dispatched head has no explicit confirmation.
    Unresolved,
    /// Intake is blocked by a sticky local incident.
    Stopped(StreamStop),
}
impl std::fmt::Display for SupervisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream supervision rejected: {self:?}")
    }
}
impl std::error::Error for SupervisionError {}

/// One pinned feed/epoch/provider declaration and explicit pending-record ceiling.
/// This is not an authority, calendar, condition or continuity qualification.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SupervisedSource {
    feed: FeedId,
    epoch: FeedEpoch,
    provider: ProviderCapability,
    capacity: NonZeroU16,
}
impl SupervisedSource {
    /// Define one lane; an unresolved dispatch counts against its capacity.
    /// # Errors
    /// Rejects capacities above the portable 4096-record ceiling.
    pub fn new(
        feed: FeedId,
        epoch: FeedEpoch,
        provider: ProviderCapability,
        capacity: NonZeroU16,
    ) -> Result<Self, SupervisionError> {
        if capacity.get() > 4096 {
            return Err(SupervisionError::Configuration);
        }
        Ok(Self {
            feed,
            epoch,
            provider,
            capacity,
        })
    }
}

/// Rejected intake retains the exact submitted observation for explicit handling.
pub struct RejectedStreamObservation {
    error: SupervisionError,
    observation: Box<CanonicalObservation>,
}
impl RejectedStreamObservation {
    /// Stable reason, without licensed payloads or source identifiers.
    #[must_use]
    pub const fn reason(&self) -> SupervisionError {
        self.error
    }
    /// Recover ownership; rejection never silently discards the submitted value.
    #[must_use]
    pub fn into_observation(self) -> CanonicalObservation {
        *self.observation
    }
}
impl std::fmt::Display for RejectedStreamObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for RejectedStreamObservation {}

/// Local occupancy and dispatch state. None of these fields establishes evidence readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamLaneStatus {
    /// Includes the unresolved FIFO head, even after its dispatch handle was dropped.
    pub pending: usize,
    /// A head was dispatched and has not been explicitly confirmed.
    pub unresolved: bool,
    /// Sticky incident, independent of unresolved external work.
    pub stopped: Option<StreamStop>,
}
struct Lane {
    source: SupervisedSource,
    queue: VecDeque<Arc<CanonicalObservation>>,
    unresolved: bool,
    stopped: Option<StreamStop>,
}

/// Non-cloneable local dispatch, never a durable receipt or account-slot release.
/// Dropping it retains the queued head and leaves the source unresolved; no retry runs.
pub struct StreamDispatch {
    owner: Arc<()>,
    lane: usize,
    feed: FeedId,
    epoch: FeedEpoch,
    observation: Arc<CanonicalObservation>,
    tail: Vec<Arc<CanonicalObservation>>,
}
impl StreamDispatch {
    /// Exact pinned feed and epoch; not a qualified source sequence.
    #[must_use]
    pub const fn identity(&self) -> (FeedId, FeedEpoch) {
        (self.feed, self.epoch)
    }
    /// Immutable FIFO head; no new receipt time or identity is assigned on dispatch.
    #[must_use]
    pub fn observation(&self) -> &CanonicalObservation {
        &self.observation
    }
    /// Exact FIFO batch, including the head; original arrivals are never reordered.
    pub fn observations(&self) -> impl Iterator<Item = &CanonicalObservation> {
        std::iter::once(self.observation.as_ref()).chain(self.tail.iter().map(AsRef::as_ref))
    }
}

/// Bounded, synchronous round-robin scheduling of already canonical caller inputs.
///
/// At most one head per source is unresolved. A slow source cannot monopolize other
/// lanes. Scheduling fairness is measured in calls, not wall-clock completion time.
/// Domain observations have bounded values/condition codes; no arbitrary provider
/// byte buffer is admitted here. Producers must separately bound their own intake.
/// This transient object has no reset, retry, reconnect or readiness-authorizing API.
pub struct StreamSupervisor {
    owner: Arc<()>,
    lanes: Vec<Lane>,
    capacity: usize,
    pending: usize,
    cursor: usize,
    cancellation: Cancellation,
}
impl StreamSupervisor {
    /// Pin 1..=32 distinct feeds, per-lane bounds and a positive aggregate ceiling.
    /// # Errors
    /// Rejects duplicates or aggregate capacity above 4096 before allocating queues.
    pub fn new(
        sources: &[SupervisedSource],
        capacity: NonZeroU16,
        cancellation: Cancellation,
    ) -> Result<Self, SupervisionError> {
        if sources.is_empty()
            || sources.len() > 32
            || capacity.get() > 4096
            || sources
                .iter()
                .enumerate()
                .any(|(i, source)| sources.iter().take(i).any(|old| old.feed == source.feed))
        {
            return Err(SupervisionError::Configuration);
        }
        Ok(Self {
            owner: Arc::new(()),
            lanes: sources
                .iter()
                .map(|source| Lane {
                    source: *source,
                    queue: VecDeque::new(),
                    unresolved: false,
                    stopped: None,
                })
                .collect(),
            capacity: usize::from(capacity.get()),
            pending: 0,
            cursor: 0,
            cancellation,
        })
    }

    /// Enqueue without eviction. Duplicate observations remain distinct arrivals.
    /// Semantic deduplication/continuity belongs to the domain tracker, not this FIFO.
    /// # Errors
    /// Returns the exact input on unknown/stale binding, cancellation, stop or overload.
    /// Binding/overload incidents latch on the affected source; other lanes can drain.
    pub fn enqueue(
        &mut self,
        feed: FeedId,
        epoch: FeedEpoch,
        observation: CanonicalObservation,
    ) -> Result<(), RejectedStreamObservation> {
        let result = self.enqueue_checked(feed, epoch, &observation);
        match result {
            Ok(lane) => {
                lane.queue.push_back(Arc::new(observation));
                self.pending += 1;
                Ok(())
            }
            Err(error) => Err(RejectedStreamObservation {
                error,
                observation: Box::new(observation),
            }),
        }
    }
    fn enqueue_checked(
        &mut self,
        feed: FeedId,
        epoch: FeedEpoch,
        observation: &CanonicalObservation,
    ) -> Result<&mut Lane, SupervisionError> {
        self.check_cancellation()?;
        let lane = self
            .lanes
            .iter_mut()
            .find(|lane| lane.source.feed == feed)
            .ok_or(SupervisionError::UnknownSource)?;
        if let Some(stop) = lane.stopped {
            return Err(SupervisionError::Stopped(stop));
        }
        let stop = if lane.source.epoch != epoch
            || lane.source.provider != observation.context().origin().1
        {
            Some(StreamStop::Binding)
        } else if lane.queue.len() >= usize::from(lane.source.capacity.get())
            || self.pending >= self.capacity
        {
            Some(StreamStop::Overload)
        } else {
            None
        };
        if let Some(stop) = stop {
            lane.stopped = Some(stop);
            return Err(SupervisionError::Stopped(stop));
        }
        Ok(lane)
    }

    /// Dispatch one eligible head and rotate the cursor. Busy/stopped/empty lanes
    /// are skipped; each continuously eligible lane gets a turn within one rotation.
    /// # Errors
    /// Cancellation stops all new dispatch. `None` means no eligible head, not health.
    pub fn dispatch_next(&mut self) -> Result<Option<StreamDispatch>, SupervisionError> {
        self.check_cancellation()?;
        let count = self.lanes.len();
        for offset in 0..count {
            let index = (self.cursor + offset) % count;
            let Some(lane) = self.lanes.get_mut(index) else {
                continue;
            };
            if lane.stopped.is_some() || lane.unresolved {
                continue;
            }
            let Some(observation) = lane.queue.front() else {
                continue;
            };
            lane.unresolved = true;
            self.cursor = (index + 1) % count;
            return Ok(Some(StreamDispatch {
                owner: Arc::clone(&self.owner),
                lane: index,
                feed: lane.source.feed,
                epoch: lane.source.epoch,
                observation: Arc::clone(observation),
                tail: Vec::new(),
            }));
        }
        Ok(None)
    }

    /// Extend an unresolved head to an explicit positive bounded FIFO batch. This groups
    /// complete frames without guessing source semantics or changing occupancy/fairness.
    /// # Errors
    /// Denies foreign/changed/stopped heads, repeated extension or count beyond the queue.
    pub fn extend_dispatch(
        &mut self,
        mut dispatch: StreamDispatch,
        count: NonZeroU16,
    ) -> Result<StreamDispatch, SupervisionError> {
        self.check_cancellation()?;
        let lane = self
            .lanes
            .get(dispatch.lane)
            .ok_or(SupervisionError::ForeignDispatch)?;
        if !Arc::ptr_eq(&self.owner, &dispatch.owner)
            || !lane.unresolved
            || lane.stopped.is_some()
            || !dispatch.tail.is_empty()
            || usize::from(count.get()) > lane.queue.len()
            || lane
                .queue
                .front()
                .is_none_or(|v| !Arc::ptr_eq(v, &dispatch.observation))
        {
            return Err(SupervisionError::ForeignDispatch);
        }
        dispatch.tail = lane
            .queue
            .iter()
            .skip(1)
            .take(usize::from(count.get()) - 1)
            .cloned()
            .collect();
        Ok(dispatch)
    }

    /// Retire only this exact local head after the caller explicitly confirms work.
    /// This trusts the caller's assertion; it proves no database/storage/broker commit.
    /// # Errors
    /// Foreign handles never mutate this supervisor. Stops/cancellation retain the head
    /// and uncertainty, including when downstream effects may already have committed.
    pub fn confirm(&mut self, dispatch: StreamDispatch) -> Result<(), SupervisionError> {
        if !Arc::ptr_eq(&self.owner, &dispatch.owner) {
            return Err(SupervisionError::ForeignDispatch);
        }
        self.check_cancellation()?;
        let lane = self
            .lanes
            .get_mut(dispatch.lane)
            .ok_or(SupervisionError::ForeignDispatch)?;
        if !lane.unresolved
            || lane
                .queue
                .front()
                .is_none_or(|head| !Arc::ptr_eq(head, &dispatch.observation))
        {
            return Err(SupervisionError::ForeignDispatch);
        }
        if lane.queue.len() < dispatch.tail.len() + 1
            || !lane
                .queue
                .iter()
                .skip(1)
                .take(dispatch.tail.len())
                .zip(&dispatch.tail)
                .all(|(a, b)| Arc::ptr_eq(a, b))
        {
            return Err(SupervisionError::ForeignDispatch);
        }
        if let Some(stop) = lane.stopped {
            return Err(SupervisionError::Stopped(stop));
        }
        lane.queue.pop_front();
        for _ in &dispatch.tail {
            lane.queue.pop_front();
        }
        lane.unresolved = false;
        self.pending -= 1 + dispatch.tail.len();
        Ok(())
    }

    /// Latch an explicit source incident without evicting records or releasing work.
    /// Repeated reports preserve the first reason; unrelated lanes may continue.
    /// # Errors
    /// Rejects an unknown source without changing another lane.
    pub fn stop_source(
        &mut self,
        feed: FeedId,
        reason: StreamStop,
    ) -> Result<(), SupervisionError> {
        let lane = self
            .lanes
            .iter_mut()
            .find(|lane| lane.source.feed == feed)
            .ok_or(SupervisionError::UnknownSource)?;
        lane.stopped.get_or_insert(reason);
        Ok(())
    }

    /// Read local occupancy. Cancellation is visible even before the next mutation.
    /// # Errors
    /// Returns `UnknownSource` for an unregistered feed.
    pub fn status(&self, feed: FeedId) -> Result<StreamLaneStatus, SupervisionError> {
        let lane = self
            .lanes
            .iter()
            .find(|lane| lane.source.feed == feed)
            .ok_or(SupervisionError::UnknownSource)?;
        Ok(StreamLaneStatus {
            pending: lane.queue.len(),
            unresolved: lane.unresolved,
            stopped: lane.stopped.or_else(|| {
                self.cancellation
                    .is_cancelled()
                    .then_some(StreamStop::Cancelled)
            }),
        })
    }

    /// Check only transient intake/dispatch health. Success on an empty supervisor
    /// is valid and explicitly grants NO evidence readiness, freshness or authority.
    /// # Errors
    /// Reports cancellation, sticky incidents, then unresolved work in that order.
    pub fn check_health(&self) -> Result<(), SupervisionError> {
        if self.cancellation.is_cancelled() {
            return Err(SupervisionError::Stopped(StreamStop::Cancelled));
        }
        if let Some(stop) = self.lanes.iter().find_map(|lane| lane.stopped) {
            return Err(SupervisionError::Stopped(stop));
        }
        if self.lanes.iter().any(|lane| lane.unresolved) {
            return Err(SupervisionError::Unresolved);
        }
        Ok(())
    }
    fn check_cancellation(&mut self) -> Result<(), SupervisionError> {
        if self.cancellation.is_cancelled() {
            for lane in &mut self.lanes {
                lane.stopped.get_or_insert(StreamStop::Cancelled);
            }
            return Err(SupervisionError::Stopped(StreamStop::Cancelled));
        }
        Ok(())
    }
}
macro_rules! redact { ($($t:ty),*) => { $(
    impl std::fmt::Debug for $t {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(concat!(stringify!($t), "(..)"))
        }
    }
)* }; }
redact!(
    SupervisedSource,
    RejectedStreamObservation,
    StreamDispatch,
    StreamSupervisor
);
