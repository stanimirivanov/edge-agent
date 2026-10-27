//! Finite capture admission and accounting, not a vendor transport or readiness grant.
mod authority;
mod inputs;
mod publication;
mod recovery;
mod segment;
use crate::{
    AccessScope, AuthorityCatalogs, BudgetError as Error, BudgetErrorKind as K, Cancellation,
    ProviderAccountId, SelectionPlan,
};
use edgeagent_contracts::evidence::{EvidenceDigest, EvidenceId};
use edgeagent_market_data_domain::{
    DeliveryCapability, EffectiveInterval, EvidenceContract, UtcInstant,
};
pub use inputs::*;
pub use publication::*;
pub use recovery::*;
pub use segment::{CapturedStreamObservation, StreamSegmentInput, freeze_stream_segment};
use std::{
    future::Future,
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
};

type Result<T> = std::result::Result<T, Error>;
fn invalid() -> Error {
    Error::new(K::Invalid)
}

/// Owner-reviewed exclusive streaming allocation; not inferred vendor-plan entitlements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingAccountPolicy {
    /// Immutable review revision; retries cannot install a fresh counter namespace.
    pub revision: NonZeroU32,
    /// Maximum simultaneously unresolved stream connection starts across replicas.
    pub connections: NonZeroU16,
    /// Lifetime connection-start allocation, including failed/unknown dials.
    pub starts: NonZeroU32,
    /// Positive maximum duration of each admitted session, in milliseconds.
    pub session_ms: NonZeroU32,
    /// Positive minimum account-wide spacing between connection starts.
    pub spacing_ms: NonZeroU32,
    /// Exclusive expiry; expiry never releases unresolved slots or resets counters.
    pub expires_at: UtcInstant,
}

/// Explicit session ceilings. All control, duplicate and unselected frames consume work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingLimits {
    /// Total starts including the initial connection; reconnects consume this same limit.
    pub starts: NonZeroU32,
    /// Maximum received frame count before stopping intake.
    pub frames: NonZeroU32,
    /// Maximum frame bytes accepted by the future bounded transport.
    pub frame_bytes: NonZeroU32,
    /// Total received bytes; one detected overage is retained, never refunded.
    pub bytes: NonZeroU64,
    /// Maximum explicitly selected instruments, no more than the existing 32-member ceiling.
    pub subscriptions: NonZeroU16,
    /// Explicit connection timeout for the future transport; not a quiescence timeout.
    pub connect_ms: NonZeroU32,
    /// Explicit idle timeout for the future transport; cannot renew market freshness.
    pub idle_ms: NonZeroU32,
}

/// Complete owner-selected capture input, distinct from daily-v1 and public event contracts.
#[derive(Clone)]
pub struct StreamingSessionData {
    /// Stable session intent; exact retries preserve all accounting.
    pub id: EvidenceId,
    /// Strictly instrument-sorted scopes sharing principal/account/source/endpoint/audience/storage.
    pub scopes: Vec<AccessScope>,
    /// One exact intraday, trade, index or option contract; no implicit coverage conversion.
    pub contract: EvidenceContract,
    /// Explicit delivery requirement, not proof of gap-free continuity.
    pub delivery: DeliveryCapability,
    /// Inclusive start and exclusive end. No daily-command ceiling is inherited.
    pub window: EffectiveInterval,
    /// Full operator configuration pin; neither this nor a checksum authenticates the operator.
    pub configuration: EvidenceDigest,
    /// Required finite resource ceilings.
    pub limits: StreamingLimits,
}

/// Sealed bounded immutable inputs and exact authority/selection witnesses; not live permission.
#[derive(Clone)]
pub struct StreamingSession {
    account: ProviderAccountId,
    data: StreamingSessionData,
    authority: serde_json::Value,
    bytes: Vec<u8>,
}
impl std::fmt::Debug for StreamingSessionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamingSessionData(..)")
    }
}
impl std::fmt::Debug for StreamingSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamingSession(..)")
    }
}
impl StreamingSession {
    /// Seal a finite exact selection and current reviewed authority without I/O or approval.
    /// # Errors
    /// Rejects invalid limits, unsupported contracts, incomplete selection or denied authority.
    pub fn new(
        data: StreamingSessionData,
        selection: &SelectionPlan<'_>,
        catalogs: AuthorityCatalogs<'_>,
        at: UtcInstant,
    ) -> Result<Self> {
        authority::seal(data, selection, catalogs, at)
    }
    /// Borrow the exact checked inputs; callers cannot mutate a sealed admission.
    #[must_use]
    pub const fn data(&self) -> &StreamingSessionData {
        &self.data
    }
    /// Private canonical record, capped at 64 KiB; not a public wire format or raw frame.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Recheck current exact revisions, bounds and cancellation after database contention.
    /// Catalog authentication remains the caller's trusted composition responsibility.
    /// # Errors
    /// Denies expiry, cancellation, withdrawal, changed reviews or missing authority.
    pub fn recheck(
        &self,
        catalogs: AuthorityCatalogs<'_>,
        at: UtcInstant,
        cancellation: &Cancellation,
    ) -> Result<()> {
        if cancellation.is_cancelled() {
            return Err(Error::new(K::Cancelled));
        }
        if !self.data.window.contains(at) {
            return Err(Error::new(K::Deadline));
        }
        if authority::reviews(&self.data, catalogs, at)? != self.authority {
            return Err(Error::new(K::Denied));
        }
        Ok(())
    }
    /// Recheck retained-data rights without renewing the original capture window.
    /// Capture cancellation is deliberately not a retained-data withdrawal.
    /// # Errors
    /// Denies changed, revoked or unavailable current reviews.
    pub fn recheck_retained(&self, catalogs: AuthorityCatalogs<'_>, at: UtcInstant) -> Result<()> {
        if authority::reviews(&self.data, catalogs, at)? != self.authority {
            return Err(Error::new(K::Denied));
        }
        Ok(())
    }
    /// Account identity shared across all selected instruments, never an API key identity.
    #[must_use]
    pub const fn account(&self) -> ProviderAccountId {
        self.account
    }
}

/// Adapter-returned process generation. The number is not authentication or a quiescence proof.
pub struct StreamingConnection {
    session: EvidenceId,
    account: ProviderAccountId,
    generation: NonZeroU64,
    usage: StreamingUsage,
}
impl std::fmt::Debug for StreamingConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamingConnection(..)")
    }
}
impl StreamingConnection {
    /// Construct from a committed positive database generation; every use must compare it atomically.
    /// # Errors
    /// Rejects zero or generations outside PostgreSQL's signed counter range.
    pub fn from_committed(
        session: &StreamingSession,
        generation: u64,
        usage: StreamingUsage,
    ) -> Result<Self> {
        if generation > i64::MAX as u64 {
            return Err(invalid());
        }
        Ok(Self {
            session: session.data.id.clone(),
            account: session.account,
            generation: NonZeroU64::new(generation).ok_or_else(invalid)?,
            usage,
        })
    }
    /// Exact generation for independently owner-authorized reconciliation; Drop releases nothing.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation.get()
    }
    /// Exact local session/account binding; not authentication or a database check.
    #[must_use]
    pub fn matches(&self, session: &StreamingSession) -> bool {
        self.session == session.data.id && self.account == session.account
    }
    /// Counters observed at reservation, allowing the next arrival ordinal after reconnection.
    #[must_use]
    pub const fn usage(&self) -> StreamingUsage {
        self.usage
    }
}

/// Read-only counters, not a permit to use or publish market evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingUsage {
    /// Committed connection starts; reconnection never resets this count.
    pub starts: u32,
    /// Received frames, including a detected over-limit frame.
    pub frames: u64,
    /// Received bytes, including a detected overage; not provider billing certification.
    pub bytes: u64,
    /// Last confirmed frame size for exact accounting-only result recovery.
    pub last_frame_bytes: Option<u32>,
    /// Sticky stop due to cancellation or exhausted work; never releases a connection slot.
    pub stopped: bool,
}
/// An accounting result, never payload acceptance, source deduplication or continuity evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingFrame {
    /// Complete counters after this accounting operation.
    pub usage: StreamingUsage,
    /// True only when this ordinal had already committed with exactly the same size.
    pub repeated: bool,
    /// Whether this frame fit the budget; never by itself permission to accept a payload.
    pub within_limits: bool,
}
/// Service-owned arrival identity and observed size, never a provider sequence or content ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingArrival {
    /// Monotonic session-local arrival ordinal, preserved across reconnects.
    pub ordinal: u64,
    /// Actual observed bytes; zero-byte control frames still consume one frame.
    pub bytes: u32,
}
impl StreamingLimits {
    /// Account for a local arrival ordinal before payload routing/decode. Recovery can repeat
    /// only the last confirmed ordinal/size; every distinct arrival uses a new ordinal.
    /// # Errors
    /// Rejects corrupt state, skipped/stale ordinals and work after a sticky stop. Detected
    /// size/count overages are charged and return stopped state, not a refunding error.
    pub fn charge(self, usage: StreamingUsage, ordinal: u64, bytes: u32) -> Result<StreamingFrame> {
        if usage.starts == 0
            || usage.starts > self.starts.get()
            || usage.frames > u64::from(self.frames.get())
            || usage.bytes > self.bytes.get().saturating_add(u64::from(u32::MAX))
            || (usage.frames == 0) != usage.last_frame_bytes.is_none()
            || (usage.frames == 0 && usage.bytes != 0)
            || usage
                .last_frame_bytes
                .is_some_and(|last| u64::from(last) > usage.bytes)
            || (!usage.stopped
                && (usage.frames >= u64::from(self.frames.get())
                    || usage.bytes >= self.bytes.get()
                    || usage
                        .last_frame_bytes
                        .is_some_and(|last| last > self.frame_bytes.get())))
        {
            return Err(invalid());
        }
        if ordinal == usage.frames && ordinal != 0 {
            if usage.last_frame_bytes != Some(bytes) {
                return Err(Error::new(K::Conflict));
            }
            return Ok(StreamingFrame {
                usage,
                repeated: true,
                within_limits: usage.bytes <= self.bytes.get() && bytes <= self.frame_bytes.get(),
            });
        }
        if usage.stopped {
            return Err(Error::new(K::Exhausted));
        }
        if ordinal != usage.frames.checked_add(1).ok_or_else(invalid)? {
            return Err(Error::new(K::Conflict));
        }
        let total = usage
            .bytes
            .checked_add(u64::from(bytes))
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(invalid)?;
        let within_limits = bytes <= self.frame_bytes.get() && total <= self.bytes.get();
        Ok(StreamingFrame {
            repeated: false,
            within_limits,
            usage: StreamingUsage {
                frames: ordinal,
                bytes: total,
                last_frame_bytes: Some(bytes),
                stopped: ordinal >= u64::from(self.frames.get())
                    || total >= self.bytes.get()
                    || !within_limits,
                ..usage
            },
        })
    }
}

/// Trusted composition supplies current authenticated references and UTC time without I/O.
pub trait StreamingAuthority: Send + Sync {
    /// Current authenticated complete rights/provider histories, including withdrawals.
    fn catalogs(&self) -> AuthorityCatalogs<'_>;
    /// Trusted owner UTC time, never a provider timestamp or a message-supplied clock.
    fn now(&self) -> UtcInstant;
}

/// Worker-owned substitution boundary for transport; no owner approval or cleanup API.
pub trait StreamingLedger: Send + Sync {
    /// Commit capacity/start debit before dialing. Unknown results retain ownership.
    /// # Errors
    /// Rejects unavailable capacity, exhausted allocations, stale authority and storage errors.
    fn reserve(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
    ) -> impl Future<Output = Result<StreamingConnection>> + Send;
    /// Commit each arrival before decode. A stopped result must halt intake immediately.
    /// # Errors
    /// Rejects stale generations, invalid ordinals, revoked authority and storage errors.
    fn charge(
        &self,
        session: &StreamingSession,
        connection: &StreamingConnection,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
        arrival: StreamingArrival,
    ) -> impl Future<Output = Result<StreamingFrame>> + Send;
}
