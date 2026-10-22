use crate::{AccessScope, AcquisitionError as Error, AcquisitionId};
use edgeagent_market_data_domain::{DateRange, EvidenceContract, InstrumentId, UtcInstant};
use std::{
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Knowledge requirement, never inferred from the dates being fetched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnowledgeMode {
    /// Current capture; knowledge is no earlier than actual input receipt.
    Capture,
    /// Previously retained inputs known at this inclusive cutoff; never backdate a fresh fetch.
    RetainedAt(UtcInstant),
}
/// Raw bytes are discarded unless explicitly requested and separately licensed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawRetention {
    /// Do not retain raw response bytes.
    Discard,
    /// Request licensed raw retention, capped independently by the grant.
    Retain,
}
/// Explicit per-operation ceilings, not durable cross-replica budget accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestBudget {
    /// Positive connect timeout, at most 5000 milliseconds.
    pub connect_ms: NonZeroU32,
    /// Positive whole-request timeout, connect included, at most 15000 milliseconds.
    pub request_ms: NonZeroU32,
    /// Remaining positive command-wide external calls, including references/failures; at most 12.
    pub calls: NonZeroU32,
    /// Remaining positive pages for this traversal, at most four.
    pub pages: NonZeroU32,
    /// Maximum decoded response bytes, at most 1 MiB.
    pub response_bytes: NonZeroU32,
    /// Remaining decoded command bytes, at most 8 MiB.
    pub command_bytes: NonZeroU32,
}
/// Immutable request configuration; admission validates before invoking any provider port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcquisitionRequest {
    /// Service-owned immutable command identity.
    pub id: AcquisitionId,
    /// Exact authenticated and licensed scope; no wildcard matching.
    pub scope: AccessScope,
    /// Requested raw daily semantics, never adjusted or silently converted.
    pub contract: EvidenceContract,
    /// Half-open request of at most 366 calendar days; PR4 checks 252 expected sessions.
    pub range: DateRange,
    /// Explicit capture/historical requirement.
    pub knowledge: KnowledgeMode,
    /// Durable initial admission time, not reset on worker restart.
    pub admitted_at: UtcInstant,
    /// Exclusive command deadline, at most 300 seconds after initial admission.
    pub deadline: UtcInstant,
    /// Explicit remaining limits supplied by the future durable budget owner.
    pub budget: RequestBudget,
    /// Raw retention intent, never implied by canonical-storage permission.
    pub raw: RawRetention,
}
impl AcquisitionRequest {
    pub(crate) fn validate(
        &self,
        now: UtcInstant,
        cancellation: &Cancellation,
    ) -> Result<(), Error> {
        if cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if now >= self.deadline {
            return Err(Error::Deadline);
        }
        let b = self.budget;
        if now < self.admitted_at
            || self.deadline.unix_millis() - self.admitted_at.unix_millis() > 300_000
            || self.range.calendar_days() > 366
            || b.connect_ms > b.request_ms
            || b.connect_ms.get() > 5000
            || b.request_ms.get() > 15000
            || b.calls.get() > 12
            || b.pages.get() > 4
            || b.response_bytes.get() > 1_048_576
            || b.command_bytes.get() > 8_388_608
            || b.response_bytes > b.command_bytes
            || matches!(self.knowledge, KnowledgeMode::RetainedAt(cutoff) if cutoff > now)
            || self.contract.shape().0
                != edgeagent_market_data_domain::EvidenceKind::RawBar(
                    edgeagent_market_data_domain::BarCoverage::ProviderDay,
                )
        {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }
    /// Requested canonical instrument, independent of provider alias.
    #[must_use]
    pub const fn instrument(&self) -> InstrumentId {
        self.scope.instrument
    }
}
/// Shared cooperative cancellation. Dropping a future never proves remote cancellation.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    /// Stop new work; adapters must check between pages/operations and bound in-flight I/O.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
