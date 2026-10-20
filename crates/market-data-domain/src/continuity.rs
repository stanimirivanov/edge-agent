//! Finite feed state; orchestration, source qualification and durable journals stay outside.
mod ingest;
mod recovery;

use crate::{
    CanonicalObservation, FeedEpoch, FeedId, ObservationId, ObservationRevision, PreciseInstant,
    ProviderCapability, SemanticReference, SessionDate, TransformationRevision,
};
use std::{collections::BTreeMap, time::Duration};

/// Payload-free continuity and readiness rejections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContinuityError {
    /// Required policy bounds or plan cardinality are invalid.
    Policy,
    /// Supplied epoch is stale or a reset does not advance it.
    Epoch,
    /// Observation belongs to another source/contract/session/transformation.
    Scope,
    /// Caller time regressed or evidence is not yet received.
    Clock,
    /// Identity or sequence was reused with different immutable content.
    Conflict,
    /// A correction lacks its current predecessor or changes instrument identity.
    Predecessor,
    /// Distinct current raw bars overlap in the same instrument/coverage.
    Overlap,
    /// Finite retained window is full; nothing was silently evicted.
    Capacity,
    /// Advancing the source sequence would wrap.
    SequenceOverflow,
    /// Gap or explicit reset still requires fenced recovery completion.
    Recovering,
    /// A conflict requires an explicit new epoch, not another retry.
    Halted,
    /// Recovery attempt budget is exhausted; resets do not refund it.
    Exhausted,
    /// Recovery is already in flight.
    RecoveryInFlight,
    /// Ticket was completed, abandoned, expired or fenced by reset/new work.
    StaleRecovery,
    /// Recovery row-work limit was exceeded, including duplicate overlap.
    RecoveryWork,
    /// A known sequence hole remains.
    Gap,
    /// No explicit heartbeat has established current feed activity.
    HeartbeatMissing,
    /// Heartbeat exceeded the explicit age bound.
    HeartbeatGap,
    /// Watermark regressed within the same epoch.
    Watermark,
    /// Snapshot-only evidence cannot prove required continuity.
    ContinuityUnknown,
    /// Required input is absent, duplicated, unaccepted or from a conflicting feed owner.
    Input,
    /// An observation has been superseded or cancelled.
    Superseded,
    /// Required source or quote-side time is too old.
    Stale,
    /// Required source/side times exceed the explicit skew bound.
    Skew,
}
impl std::fmt::Display for ContinuityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Variant names are fixed, payload-free categories, not provider error text.
        write!(f, "evidence continuity rejected: {self:?}")
    }
}
impl std::error::Error for ContinuityError {}

/// Caller-qualified sequence interpretation; no source counter implies gap freedom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContinuityMode {
    /// Independent observations; no continuity claim.
    Snapshots,
    /// Complete declared stream with gap-free sequence, starting at an explicit baseline.
    Contiguous {
        /// First expected sequence; zero is valid.
        next: u64,
    },
}
/// State as of the last mutation; current-time readiness also checks heartbeat age.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContinuityState {
    /// Tracking without a known unresolved incident; not itself a readiness grant.
    Tracking,
    /// An incident remains fenced until explicit recovery completion.
    Recovering,
    /// Conflicting evidence requires a new epoch.
    Halted,
    /// Capacity, sequence or lifetime attempt limit stopped intake.
    Exhausted,
}
/// Explicit finite limits; no timing default selects or qualifies a provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContinuityPolicy {
    revision: SemanticReference,
    capacity: usize,
    attempts: u16,
    heartbeat_age: Duration,
    recovery_age: Duration,
}
impl ContinuityPolicy {
    /// Require 1..=4096 retained records, 1..=64 lifetime attempts and nonzero
    /// heartbeat/recovery durations. Capacity also bounds row work per attempt.
    ///
    /// # Errors
    /// Returns `Policy` for missing/unsupported bounds. Source/account qualification is external.
    pub fn new(
        revision: SemanticReference,
        capacity: usize,
        attempts: u16,
        heartbeat_age: Duration,
        recovery_age: Duration,
    ) -> Result<Self, ContinuityError> {
        if !(1..=4096).contains(&capacity)
            || !(1..=64).contains(&attempts)
            || heartbeat_age.is_zero()
            || recovery_age.is_zero()
        {
            return Err(ContinuityError::Policy);
        }
        Ok(Self {
            revision,
            capacity,
            attempts,
            heartbeat_age,
            recovery_age,
        })
    }
    /// Exact reviewed policy identity; it is not a data-rights grant.
    #[must_use]
    pub const fn revision(self) -> SemanticReference {
        self.revision
    }
}
/// Complete sequence scope may contain multiple instruments under one exact
/// contract. Filtered global counters must not be called gap-free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedScope {
    id: FeedId,
    provider: ProviderCapability,
    transformation: TransformationRevision,
    session: SessionDate,
}
impl FeedScope {
    /// Bind the caller-owned stream identity to a checked source/semantic scope.
    /// Qualify routing, session and sequence meaning before choosing contiguous mode.
    #[must_use]
    pub const fn new(
        id: FeedId,
        provider: ProviderCapability,
        transformation: TransformationRevision,
        session: SessionDate,
    ) -> Self {
        Self {
            id,
            provider,
            transformation,
            session,
        }
    }
    /// Stream identity and source declaration; no implicit provider substitution.
    #[must_use]
    pub const fn binding(self) -> (FeedId, ProviderCapability) {
        (self.id, self.provider)
    }
}
/// Observable intake result; released count includes earlier buffered records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IngestOutcome {
    /// Newly accepted prefix/snapshot records.
    Accepted {
        /// Number of newly accepted records.
        released: usize,
    },
    /// Retained out of order; never eligible for readiness yet.
    Buffered,
    /// Same immutable source revision; first receipt and freshness are unchanged.
    Duplicate,
}
/// Fenced recovery handle, not authorization or an external-work guarantee.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryTicket {
    feed: FeedId,
    epoch: FeedEpoch,
    attempt: u16,
}
struct ActiveRecovery {
    ticket: RecoveryTicket,
    started: PreciseInstant,
    rows: usize,
}

/// Bounded in-memory state machine. Reconstruct from an owned journal or persist
/// its epoch/budget in the later application layer; this is not durable execution.
///
/// ```
/// use edgeagent_market_data_domain::*;
/// use std::time::Duration;
/// let at = PreciseInstant::from_millis(UtcInstant::from_unix_millis(0)?);
/// let rules = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
/// let contract = EvidenceContract::new(EvidenceKind::IndexLevel,
///     MarketScope::IndexPublisher, rules, TimestampPrecision::Nanoseconds)?;
/// let provider = ProviderCapability::new(SourceId::new(1)?, contract,
///     DeliveryCapability::Snapshots, KnowledgeCapability::CaptureOnly);
/// let scope = FeedScope::new(FeedId::new(1)?, provider,
///     TransformationRevision::new(1)?, SessionDate::new(1970, 1, 1)?);
/// let limits = ContinuityPolicy::new(rules, 8, 2,
///     Duration::from_secs(1), Duration::from_secs(5))?;
/// let mut tracker = ContinuityTracker::new(scope, FeedEpoch::new(1)?,
///     ContinuityMode::Snapshots, limits, at)?;
/// tracker.reset(FeedEpoch::new(2)?, ContinuityMode::Snapshots, at)?;
/// let ticket = tracker.begin_recovery(FeedEpoch::new(2)?, at)?;
/// tracker.heartbeat(FeedEpoch::new(2)?, at, None)?;
/// tracker.finish_recovery(ticket, at)?;
/// // Tracking alone is not evidence readiness: no observations exist yet.
/// assert_eq!(tracker.status(), (ContinuityState::Tracking, 0, 1));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct ContinuityTracker {
    scope: FeedScope,
    epoch: FeedEpoch,
    mode: ContinuityMode,
    policy: ContinuityPolicy,
    state: ContinuityState,
    clock: PreciseInstant,
    started: PreciseInstant,
    heartbeat: Option<PreciseInstant>,
    watermark: Option<u64>,
    attempts: u16,
    active: Option<ActiveRecovery>,
    records: BTreeMap<u64, CanonicalObservation>,
    versions: BTreeMap<(ObservationId, ObservationRevision), u64>,
    current: BTreeMap<ObservationId, u64>,
}
impl ContinuityTracker {
    /// Start a finite scope at an explicit time/baseline, without assuming activity.
    ///
    /// # Errors
    /// Returns `ContinuityUnknown` if contiguous mode lacks a sequenced declaration.
    pub fn new(
        scope: FeedScope,
        epoch: FeedEpoch,
        mode: ContinuityMode,
        policy: ContinuityPolicy,
        at: PreciseInstant,
    ) -> Result<Self, ContinuityError> {
        Self::validate_mode(scope, mode)?;
        Ok(Self {
            scope,
            epoch,
            mode,
            policy,
            state: ContinuityState::Tracking,
            clock: at,
            started: at,
            heartbeat: None,
            watermark: None,
            attempts: 0,
            active: None,
            records: BTreeMap::new(),
            versions: BTreeMap::new(),
            current: BTreeMap::new(),
        })
    }
    fn validate_mode(scope: FeedScope, mode: ContinuityMode) -> Result<(), ContinuityError> {
        if matches!(mode, ContinuityMode::Contiguous { .. })
            && scope.provider.declaration().1 != crate::DeliveryCapability::Sequenced
        {
            return Err(ContinuityError::ContinuityUnknown);
        }
        Ok(())
    }
    /// Scope, epoch and reviewed continuity policy.
    #[must_use]
    pub const fn identity(&self) -> (FeedScope, FeedEpoch, SemanticReference) {
        (self.scope, self.epoch, self.policy.revision)
    }
    /// Last-mutated state, retained row count and lifetime attempts consumed.
    #[must_use]
    pub fn status(&self) -> (ContinuityState, usize, u16) {
        (self.state, self.records.len(), self.attempts)
    }
    pub(crate) fn mode_is_snapshot(&self) -> bool {
        self.mode == ContinuityMode::Snapshots
    }
    fn epoch(&self, epoch: FeedEpoch) -> Result<(), ContinuityError> {
        if epoch != self.epoch {
            return Err(ContinuityError::Epoch);
        }
        Ok(())
    }
    fn advance_clock(&mut self, at: PreciseInstant) -> Result<(), ContinuityError> {
        if at < self.clock {
            return Err(ContinuityError::Clock);
        }
        if elapsed(at, self.heartbeat.unwrap_or(self.started))
            .is_some_and(|age| age > self.policy.heartbeat_age.as_nanos())
            && self.state == ContinuityState::Tracking
        {
            self.state = ContinuityState::Recovering;
        }
        if self.active.as_ref().is_some_and(|active| {
            elapsed(at, active.started).is_some_and(|age| age > self.policy.recovery_age.as_nanos())
        }) {
            self.active = None;
        }
        self.clock = at;
        Ok(())
    }
    pub(crate) fn health(&self, at: PreciseInstant) -> Result<(), ContinuityError> {
        if at < self.clock {
            return Err(ContinuityError::Clock);
        }
        match self.state {
            ContinuityState::Recovering => return Err(ContinuityError::Recovering),
            ContinuityState::Halted => return Err(ContinuityError::Halted),
            ContinuityState::Exhausted => return Err(ContinuityError::Exhausted),
            ContinuityState::Tracking => {}
        }
        let heartbeat = self.heartbeat.ok_or(ContinuityError::HeartbeatMissing)?;
        if elapsed(at, heartbeat).is_none_or(|age| age > self.policy.heartbeat_age.as_nanos()) {
            return Err(ContinuityError::HeartbeatGap);
        }
        self.check_watermark()
    }
    fn check_watermark(&self) -> Result<(), ContinuityError> {
        if let ContinuityMode::Contiguous { next } = self.mode
            && self.watermark.and_then(|mark| mark.checked_add(1)) != Some(next)
        {
            return Err(ContinuityError::ContinuityUnknown);
        }
        Ok(())
    }
}
impl std::fmt::Debug for ContinuityTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContinuityTracker")
            .field("state", &self.state)
            .field("records", &self.records.len())
            .field("attempts", &self.attempts)
            .finish_non_exhaustive()
    }
}
pub(crate) fn elapsed(later: PreciseInstant, earlier: PreciseInstant) -> Option<u128> {
    let (ls, ln) = later.unix_parts();
    let (es, en) = earlier.unix_parts();
    u128::try_from(
        (i128::from(ls) - i128::from(es)) * 1_000_000_000 + i128::from(ln) - i128::from(en),
    )
    .ok()
}
