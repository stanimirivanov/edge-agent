//! Restore is a durable barrier, never a replacement for domain readiness.
use super::*;
use crate::{StreamSegmentAccess, StreamSegmentStorage, StreamStop};
use edgeagent_market_data_domain::InstrumentId;

/// Persisted local-supervision state; contains no market payload or account credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamRecoveryState {
    /// Monotonic revision. Zero means no restore has been begun.
    pub revision: u64,
    /// Independent restore approval has not been granted.
    pub blocked: bool,
    /// Irreversible for this session; reconciliation requires a separately admitted session.
    pub incident: Option<StreamStop>,
}
/// Exact process-local restore witness, not a readiness or authorization grant.
/// Reconstructing a supervisor requires a new durable `begin_restore`, not a copied snapshot.
pub struct StreamRestore {
    session: EvidenceId,
    revision: u64,
}
impl StreamRestore {
    /// Trusted adapter constructor after confirmed durable restore mutation.
    /// # Errors
    /// Rejects revision zero. This constructor alone grants no database authority.
    pub fn from_committed(session: EvidenceId, revision: u64) -> Result<Self> {
        if revision == 0 {
            return Err(invalid());
        }
        Ok(Self { session, revision })
    }
    /// Opaque session and exact durable revision; neither is a bearer credential.
    #[must_use]
    pub const fn identity(&self) -> (&EvidenceId, u64) {
        (&self.session, self.revision)
    }
}
impl std::fmt::Debug for StreamRestore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamRestore(..)")
    }
}
/// Worker-owned barrier port. Independent owner approval is deliberately absent.
pub trait StreamRecoveryLedger: Send + Sync {
    /// Recheck exact restoring revision, current authority and sticky incident denial.
    /// Does not clear the barrier or claim that publication/capture proves readiness.
    fn check_restoring(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        catalogs: AuthorityCatalogs<'_>,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// Discover one bounded lane and reuse its exact inventoried preparation. No new ID,
/// clock, predecessor, bytes or event metadata is generated. Check the barrier on both
/// sides of external I/O. Successful recovery still requires independent owner approval.
/// # Errors
/// Cancellation, authority loss, stale/incident fences or uncertain I/O leave the
/// barrier and preparation intact. Never compensate, release slots or automatically retry.
#[allow(
    clippy::too_many_arguments,
    reason = "Explicit existing storage, authority and recovery boundaries."
)]
pub async fn recover_pending_stream_segment(
    ledger: &impl StreamPublicationLedger,
    recovery: &impl StreamRecoveryLedger,
    store: &impl StreamSegmentStorage,
    session: &StreamingSession,
    restore: &StreamRestore,
    instrument: InstrumentId,
    catalogs: AuthorityCatalogs<'_>,
    cancellation: &Cancellation,
    access: &impl StreamSegmentAccess,
) -> Result<Option<StreamCaptureBoundary>> {
    if restore.identity().0 != &session.data().id {
        return Err(invalid());
    }
    if cancellation.is_cancelled() {
        return Err(Error::new(K::Cancelled));
    }
    recovery.check_restoring(session, restore, catalogs).await?;
    let proposal = ledger
        .pending_segment(session, instrument, catalogs)
        .await?;
    let result = if let Some(proposal) = proposal {
        if proposal.segment().data().scope.session != session.data().id
            || proposal.segment().data().scope.instrument.as_str()
                != format!("{:032x}", instrument.get())
        {
            return Err(invalid());
        }
        Some(
            publish_stream_segment(
                ledger,
                store,
                session,
                &proposal,
                catalogs,
                cancellation,
                access,
            )
            .await?,
        )
    } else {
        None
    };
    if cancellation.is_cancelled() {
        return Err(Error::new(K::Cancelled));
    }
    recovery.check_restoring(session, restore, catalogs).await?;
    Ok(result)
}
