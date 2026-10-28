use super::*;
use edgeagent_market_data_http::{IexAcceptanceReferences, IexCanonicalProfile, IexObservation};
use std::num::NonZeroU32;
/// Trusted explicit offline clock; it never replaces source/receipt timestamps.
pub trait OfflineStreamClock: Send + Sync {
    /// Current evaluation time.
    fn now(&self) -> UtcInstant;
}
/// Operator limits, with no timing/capacity defaults or implicit retries.
#[derive(Clone, Copy, Debug)]
pub struct OfflineStreamLimits {
    /// Whole begin/run work deadline, at most 60 seconds; failed run gets one extra io-ms fencing budget.
    pub run_ms: NonZeroU32,
    /// Per mutation/publication/fencing timeout, at most 15 seconds.
    pub io_ms: NonZeroU32,
    /// Concurrent publication futures, at most the 32 registered feeds.
    pub inflight: NonZeroU16,
    /// Aggregate queued/unresolved observations, at most 4096.
    pub queue: NonZeroU16,
    /// Total input frames in one finite run, at most 4096.
    pub frames: NonZeroU16,
    /// Combined restored/new native records, at most 4096.
    pub records: NonZeroU16,
    /// Total retained publication inventory, at most 128.
    pub history: NonZeroU16,
    /// Aggregate private preparation bytes before fetching bodies, at most 16 MiB.
    pub history_bytes: NonZeroU32,
}
/// Complete typed source configuration, never an environment/vendor activation selector.
pub struct OfflineStreamSource<'a> {
    /// Owner-admitted complete session.
    pub session: &'a StreamingSession,
    /// Complete immutable IEX interpretation profile.
    pub profile: &'a IexCanonicalProfile,
    /// Full frozen selection witnesses.
    pub selection: &'a SelectionPlan<'a>,
    /// Trusted current rights/provider review histories.
    pub catalogs: AuthorityCatalogs<'a>,
    /// Trusted current mapping history.
    pub mappings: &'a ReferenceHistory<InstrumentMapping>,
    /// Trusted current calendar history.
    pub calendars: &'a ReferenceHistory<CalendarDay>,
    /// Full independently installed expected record, not a self-approving digest.
    pub inputs: &'a StreamOwnerInputs,
    /// Explicit feed/provider/transformation/date meaning.
    pub scope: FeedScope,
    /// Explicit feed epoch.
    pub epoch: FeedEpoch,
    /// Explicit snapshot continuity/activity policy.
    pub continuity: ContinuityPolicy,
    /// Per-feed pending-record bound, at most 4096.
    pub queue: NonZeroU16,
    /// Explicit canonical storage limits.
    pub segment_limits: StreamSegmentLimits,
    /// Explicit native companion storage limits.
    pub native_limits: StreamSegmentLimits,
    /// Common exclusive deadline; must not outlive original authority/approved inputs.
    pub delete_by: UtcInstant,
}
impl OfflineStreamSource<'_> {
    pub(super) fn references(&self) -> IexAcceptanceReferences<'_> {
        IexAcceptanceReferences {
            mappings: self.mappings,
            calendars: self.calendars,
        }
    }
}
impl std::fmt::Debug for OfflineStreamSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineStreamSource(..)")
    }
}
/// One explicit synthetic complete-frame arrival; controls/empty frames still consume budget.
pub struct OfflineStreamFrame {
    /// Exact next durable ordinal and synthetic observed byte count.
    pub arrival: StreamingArrival,
    /// Explicit synthetic feed-activity pulse; it never freshens market values.
    pub activity: bool,
    /// Normalized selected records from this complete frame; at most 1024.
    pub records: Vec<IexObservation>,
}
impl std::fmt::Debug for OfflineStreamFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineStreamFrame(..)")
    }
}
/// Requested retained revision; domain policy still checks availability, epoch and freshness.
#[derive(Clone, Copy, Debug)]
pub struct OfflineStreamCandidate {
    /// Explicit registered feed.
    pub feed: FeedId,
    /// Original logical source identity.
    pub id: ObservationId,
    /// Exact retained revision.
    pub revision: ObservationRevision,
}
pub(super) fn validate(sources: &[OfflineStreamSource<'_>], l: OfflineStreamLimits) -> Result<()> {
    if sources.is_empty()
        || sources.len() > 32
        || usize::from(l.inflight.get()) > sources.len()
        || l.run_ms.get() > 60000
        || l.io_ms.get() > 15000
        || l.io_ms.get() > l.run_ms.get()
        || l.queue.get() > 4096
        || l.frames.get() > 4096
        || l.records.get() > 4096
        || l.history.get() > 128
        || l.history_bytes.get() > 16 * 1024 * 1024
        || sources.iter().enumerate().any(|(i, s)| {
            s.queue.get() > 4096
                || [s.segment_limits, s.native_limits].iter().any(|v| {
                    v.bytes == 0 || v.bytes > 4 * 1024 * 1024 || v.records == 0 || v.records > 4096
                })
                || s.scope.binding().0 != s.selection.routing_scope().feed
                || s.epoch != s.selection.routing_scope().epoch
                || s.scope.binding().1
                    != ProviderCapability::new(
                        s.selection.routing_scope().source,
                        s.session.data().contract,
                        DeliveryCapability::Snapshots,
                        KnowledgeCapability::CaptureOnly,
                    )
                || s.scope.binding().1.declaration().0 != s.session.data().contract
                || s.scope.binding().1.declaration().1 != DeliveryCapability::Snapshots
                || s.delete_by > s.inputs.expires_at()
                || sources.iter().take(i).any(|old| {
                    old.scope.binding().0 == s.scope.binding().0
                        || old.session.data().id == s.session.data().id
                })
        })
    {
        return Err(OfflineStreamError::Configuration);
    }
    Ok(())
}
