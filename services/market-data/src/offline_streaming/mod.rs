//! Finite synthetic execution with independent approval and borrowed domain readiness.
mod config;
mod engine;
mod history;
mod work;
pub use config::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_http::{IexAcceptedObservation, IexReplayContext};
use edgeagent_market_data_postgres::{PostgresStreamingLedger, StreamRuntimePhase};
use std::{
    collections::{BTreeMap, VecDeque},
    num::NonZeroU16,
    time::Duration,
};
type Result<T> = std::result::Result<T, OfflineStreamError>;
/// Fixed payload-free offline execution rejection. No variant authorizes retry or release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OfflineStreamError {
    /// Invalid or unsupported explicit finite bounds/binding.
    Configuration,
    /// Actual independent approval, current inputs or authority rejected the operation.
    Authority,
    /// History is incomplete, retired, unsupported or exceeds the explicit bounds.
    History,
    /// Semantic/reference/identity acceptance failed.
    Semantics,
    /// Retained evidence conflicts or exhausted the domain continuity policy.
    Continuity,
    /// Explicit source/aggregate queue capacity rejected admission without eviction.
    Overload,
    /// A paired publication/storage/accounting result is uncertain; restore is required.
    Uncertain,
    /// A finite deadline expired; no external quiescence follows.
    Deadline,
    /// Explicit cancellation blocked work.
    Cancelled,
    /// Durable incident/fencing could not be confirmed; never expose readiness.
    Unconfirmed,
    /// Runtime/domain state cannot supply the requested point-in-time evidence.
    NotReady,
}
impl std::fmt::Display for OfflineStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "offline stream rejected: {self:?}")
    }
}
impl std::error::Error for OfflineStreamError {}
struct Retained {
    observation: IexAcceptedObservation,
    publication: EvidenceId,
    canonical: StreamSegmentObject,
    native: StreamCompanionObject,
}
struct Lane {
    restore: StreamRestore,
    tracker: Option<ContinuityTracker>,
    heads: BTreeMap<InstrumentId, EvidenceDigest>,
    native: Vec<Retained>,
    queued: VecDeque<Vec<IexAcceptedObservation>>,
    connection: Option<StreamingConnection>,
    failed: bool,
    fenced: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitApproval,
    Executing,
    Complete,
    Failed,
}
/// One-shot finite runtime. Construction always starts new durable restore revisions.
/// It never installs/approves inputs, approves restores, or releases external capacity.
pub struct OfflineStreamRuntime<'a, S, C> {
    sources: Vec<OfflineStreamSource<'a>>,
    lanes: Vec<Lane>,
    supervisor: StreamSupervisor,
    ledger: PostgresStreamingLedger,
    store: &'a S,
    clock: &'a C,
    cancellation: Cancellation,
    limits: OfflineStreamLimits,
    budget: std::sync::Arc<work::Budget>,
    phase: Phase,
}
impl<S, C> std::fmt::Debug for OfflineStreamRuntime<'_, S, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineStreamRuntime(..)")
    }
}
impl<'a, S: StreamSegmentStorage, C: OfflineStreamClock> OfflineStreamRuntime<'a, S, C> {
    /// Begin fresh blocked revisions, recover exact pending pairs and reconstruct complete
    /// bounded receipt history. The returned worker must still obtain independent approval.
    /// # Errors
    /// Leaves durable barriers/inventory intact on any timeout, denial or incomplete history.
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit offline context and independent ports; no ambient configuration."
    )]
    pub async fn begin(
        sources: Vec<OfflineStreamSource<'a>>,
        ledger: PostgresStreamingLedger,
        store: &'a S,
        clock: &'a C,
        cancellation: Cancellation,
        limits: OfflineStreamLimits,
    ) -> Result<Self> {
        config::validate(&sources, limits)?;
        if sources
            .iter()
            .any(|s| s.delete_by <= clock.now() || s.inputs.expires_at() <= clock.now())
        {
            return Err(OfflineStreamError::Authority);
        }
        let operation = async {
            let mut witnesses = Vec::with_capacity(sources.len());
            for source in &sources {
                witnesses.push(
                    ledger
                        .begin_restore(source.session, source.catalogs)
                        .await
                        .map_err(|_| OfflineStreamError::Authority)?,
                );
            }
            let bindings = sources
                .iter()
                .map(|s| {
                    SupervisedSource::new(
                        s.scope.binding().0,
                        s.epoch,
                        s.scope.binding().1,
                        s.queue,
                    )
                    .map_err(|_| OfflineStreamError::Configuration)
                })
                .collect::<Result<Vec<_>>>()?;
            let supervisor = StreamSupervisor::new(&bindings, limits.queue, cancellation.clone())
                .map_err(|_| OfflineStreamError::Configuration)?;
            let mut runtime = Self {
                sources,
                lanes: Vec::new(),
                supervisor,
                ledger,
                store,
                clock,
                cancellation,
                limits,
                budget: std::sync::Arc::new(work::Budget::new(0, 0, limits)),
                phase: Phase::AwaitApproval,
            };
            let mut rows = 0usize;
            let mut bytes = 0usize;
            let mut records = 0usize;
            for (i, restore) in witnesses.into_iter().enumerate() {
                let lane =
                    history::restore(&runtime, i, restore, &mut rows, &mut bytes, &mut records)
                        .await?;
                runtime.lanes.push(lane);
            }
            runtime.budget = std::sync::Arc::new(work::Budget::new(rows, bytes, limits));
            Ok(runtime)
        };
        tokio::time::timeout(
            Duration::from_millis(u64::from(limits.run_ms.get())),
            operation,
        )
        .await
        .map_err(|_| OfflineStreamError::Deadline)?
    }
    /// Exact independent-approval inputs; never a serializable readiness/activation token.
    pub fn restores(&self) -> impl Iterator<Item = (&StreamingSession, &StreamRestore)> {
        self.sources
            .iter()
            .zip(&self.lanes)
            .map(|(s, l)| (s.session, &l.restore))
    }
    /// Bounded local occupancy/incident diagnostics; success grants no readiness or authority.
    /// # Errors
    /// Rejects an unregistered feed.
    pub fn lane_status(&self, feed: FeedId) -> Result<StreamLaneStatus> {
        self.supervisor
            .status(feed)
            .map_err(|_| OfflineStreamError::Configuration)
    }
    async fn gate(&self, i: usize) -> Result<()> {
        let source = self
            .sources
            .get(i)
            .ok_or(OfflineStreamError::Configuration)?;
        let lane = self.lanes.get(i).ok_or(OfflineStreamError::Configuration)?;
        let operation = async {
            self.ledger
                .check_stream_restored(source.session, &lane.restore, source.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Authority)?;
            work::inputs(&self.ledger, source, self.clock, &self.cancellation).await?;
            Ok(())
        };
        tokio::time::timeout(self.io(), operation)
            .await
            .map_err(|_| OfflineStreamError::Deadline)?
    }
    fn io(&self) -> Duration {
        Duration::from_millis(u64::from(self.limits.io_ms.get()))
    }
    /// Check actual fences and current references, then return borrowed domain evidence.
    /// Queue emptiness, receipts and successful recovery are never sufficient.
    /// # Errors
    /// Denies unfinished/failed execution, missing inputs/activity, stale data, conflicts or revocation.
    pub async fn ready<'r>(
        &'r self,
        policy: &ReadinessPolicy,
        candidates: &[OfflineStreamCandidate],
    ) -> Result<ReadyEvidence<'r>> {
        tokio::time::timeout(
            Duration::from_millis(u64::from(self.limits.run_ms.get())),
            self.ready_inner(policy, candidates),
        )
        .await
        .map_err(|_| OfflineStreamError::Deadline)?
    }
    async fn ready_inner<'r>(
        &'r self,
        policy: &ReadinessPolicy,
        candidates: &[OfflineStreamCandidate],
    ) -> Result<ReadyEvidence<'r>> {
        if self.phase != Phase::Complete
            || self.cancellation.is_cancelled()
            || candidates.len() > 32
        {
            return Err(OfflineStreamError::NotReady);
        }
        self.supervisor
            .check_health()
            .map_err(|_| OfflineStreamError::NotReady)?;
        let gates = async {
            for i in 0..self.sources.len() {
                self.gate(i).await?;
            }
            Ok(())
        };
        tokio::time::timeout(
            Duration::from_millis(u64::from(self.limits.run_ms.get())),
            gates,
        )
        .await
        .map_err(|_| OfflineStreamError::Deadline)??;
        let at = self.clock.now();
        let mut inputs = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let i = self
                .sources
                .iter()
                .position(|s| s.scope.binding().0 == candidate.feed)
                .ok_or(OfflineStreamError::NotReady)?;
            let source = &self.sources[i];
            let lane = &self.lanes[i];
            let retained = lane
                .native
                .iter()
                .find(|n| {
                    let (id, rev, _) = n.observation.canonical().version().lineage();
                    id == candidate.id && rev == candidate.revision
                })
                .ok_or(OfflineStreamError::NotReady)?;
            // Cached continuity values are never a retained-data permission. Recheck the original
            // exact pair receipt, including expiry/retirement, before returning any borrowed value.
            let object = self
                .ledger
                .published_segment(source.session, &retained.publication, source.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Authority)?;
            let companion = self
                .ledger
                .published_companion(source.session, &retained.publication, source.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Authority)?
                .ok_or(OfflineStreamError::History)?;
            if companion.segment != object
                || retained.canonical != object
                || retained.native != companion
            {
                return Err(OfflineStreamError::History);
            }
            let accepted = source
                .profile
                .accept(
                    source.session,
                    source.selection,
                    source.catalogs,
                    source.references(),
                    at,
                    &self.cancellation,
                    retained.observation.native().clone(),
                )
                .map_err(|_| OfflineStreamError::Semantics)?;
            if accepted.canonical() != retained.observation.canonical() {
                return Err(OfflineStreamError::Semantics);
            }
            inputs.push(EvidenceInput::new(
                lane.tracker.as_ref().ok_or(OfflineStreamError::NotReady)?,
                source.epoch,
                candidate.id,
                candidate.revision,
            ));
        }
        if self.cancellation.is_cancelled() {
            return Err(OfflineStreamError::Cancelled);
        }
        for i in 0..self.sources.len() {
            self.gate(i).await?;
        }
        policy
            .check(PreciseInstant::from_millis(self.clock.now()), &inputs)
            .map_err(|_| OfflineStreamError::NotReady)
    }
}
