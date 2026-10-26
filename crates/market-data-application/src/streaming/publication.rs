//! Pre-PUT inventory and explicit recovery; no implicit retry or continuity inference.
mod validation;
use super::*;
use crate::{EvidenceStorageError, StreamSegmentAccess, StreamSegmentStorage};
use edgeagent_contracts::{MessageEnvelope, MessageMetadata, evidence::*};

/// Exact recoverable proposal. IDs and event metadata must be reused after uncertainty.
pub struct StreamPublication {
    id: EvidenceId,
    segment: StreamSegment,
    envelope: MessageEnvelope,
}
impl std::fmt::Debug for StreamPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamPublication(..)")
    }
}
impl StreamPublication {
    /// Bind an opaque intent and caller's trace context to the exact frozen capture.
    /// # Errors
    /// Rejects changed route, partition, source or opaque message identity.
    pub fn new(id: EvidenceId, segment: StreamSegment, metadata: MessageMetadata) -> Result<Self> {
        if metadata.subject != format!("stream/{}", segment.data().scope.session.as_str())
            || metadata.correlation_id != id.as_str()
            || metadata.causation_id != segment.data().scope.session.as_str()
            || metadata.idempotency_key != id.as_str()
        {
            return Err(invalid());
        }
        let payload = StreamPublished {
            session: segment.data().scope.session.clone(),
            publication: id.clone(),
        };
        let envelope = MessageEnvelope::from_payload(metadata, &payload).map_err(|_| invalid())?;
        STREAM_PUBLISHED
            .validate_envelope(&envelope)
            .map_err(|_| invalid())?;
        if envelope.id() != format!("stream-publication-{}", id.as_str())
            || envelope.partition_key()
                != Some(format!("stream/{}", payload.session.as_str()).as_str())
        {
            return Err(invalid());
        }
        Ok(Self {
            id,
            segment,
            envelope,
        })
    }
    /// Original opaque intent; not a checksum or bearer capability.
    #[must_use]
    pub const fn id(&self) -> &EvidenceId {
        &self.id
    }
    /// Exact immutable capture, never regenerated during recovery.
    #[must_use]
    pub const fn segment(&self) -> &StreamSegment {
        &self.segment
    }
    /// Exact opaque event, including original time and trace context.
    #[must_use]
    pub const fn envelope(&self) -> &MessageEnvelope {
        &self.envelope
    }
    /// Canonical private recovery record; contains licensed data, never log it.
    /// # Errors
    /// Rejects a serialization failure.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let event = self.envelope.to_json().map_err(|_| invalid())?;
        serde_json::to_vec(&(self.id.as_str(), self.segment.bytes(), event)).map_err(|_| invalid())
    }
    /// Decode only bounded owner-retained preparations and verify exact re-encoding.
    /// # Errors
    /// Rejects malformed, noncanonical or oversized records.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        // JSON byte arrays are private recovery encoding, not another public segment format.
        if bytes.len() > MAX_STREAM_SEGMENT_BYTES * 5 + 4096 {
            return Err(invalid());
        }
        let (id, segment, event): (String, Vec<u8>, Vec<u8>) =
            serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let segment = StreamSegment::decode(&segment, &DailyEvidenceArtifact::hash(&segment))
            .map_err(|_| invalid())?;
        let envelope = MessageEnvelope::from_json(&event).map_err(|_| invalid())?;
        let payload: StreamPublished = envelope.payload().map_err(|_| invalid())?;
        if payload.session != segment.data().scope.session || payload.publication.as_str() != id {
            return Err(invalid());
        }
        STREAM_PUBLISHED
            .validate_envelope(&envelope)
            .map_err(|_| invalid())?;
        let value = Self {
            id: EvidenceId::new(&id).map_err(|_| invalid())?,
            segment,
            envelope,
        };
        if value.envelope.id() != format!("stream-publication-{}", id)
            || value.envelope.subject()
                != Some(format!("stream/{}", payload.session.as_str()).as_str())
            || value.envelope.extension("correlationid") != Some(id.as_str())
            || value.envelope.extension("causationid") != Some(payload.session.as_str())
            || value.envelope.extension("idempotencykey") != Some(id.as_str())
            || value.envelope.partition_key()
                != Some(format!("stream/{}", payload.session.as_str()).as_str())
            || value.bytes()? != bytes
        {
            return Err(invalid());
        }
        Ok(value)
    }
}

/// Explicit local capture discontinuities. None of these certifies source continuity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamCaptureBoundary {
    /// No retained predecessor (first capture or deliberately retired history).
    pub history_start: bool,
    /// A prior capture position exists, but its licensed predecessor was retired.
    pub history_retired: bool,
    /// A connection generation or feed epoch changed.
    pub reconnect: bool,
    /// Local capture skipped one or more already charged frames.
    pub skipped: bool,
}

/// Owner's atomic preparation/receipt/checkpoint/outbox boundary.
pub trait StreamPublicationLedger: Send + Sync {
    /// Discover the lane's sole preparation after process loss, before allocating another ID
    /// or freezing input. Never scan another session or infer a replacement from a checksum.
    fn pending_segment(
        &self,
        session: &StreamingSession,
        instrument: edgeagent_market_data_domain::InstrumentId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> impl Future<Output = Result<Option<StreamPublication>>> + Send;
    /// Recover an already committed exact receipt without another PUT or capture-generation check.
    fn recover_publication(
        &self,
        session: &StreamingSession,
        proposal: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
    ) -> impl Future<Output = Result<Option<StreamCaptureBoundary>>> + Send;
    /// Inventory exact bytes before any object PUT; retries must match all original bytes.
    fn stage(
        &self,
        session: &StreamingSession,
        proposal: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
    ) -> impl Future<Output = Result<()>> + Send;
    /// Recover the original preparation by opaque identity under current authorization.
    fn recover_segment(
        &self,
        session: &StreamingSession,
        id: &EvidenceId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> impl Future<Output = Result<Option<StreamPublication>>> + Send;
    /// Commit only after verified object read-back, with all fences rechecked atomically.
    fn commit_segment(
        &self,
        session: &StreamingSession,
        proposal: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
    ) -> impl Future<Output = Result<StreamCaptureBoundary>> + Send;
    /// Resolve a retained published object under durable denial and original expiry.
    fn published_segment(
        &self,
        session: &StreamingSession,
        id: &EvidenceId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> impl Future<Output = Result<StreamSegmentObject>> + Send;
}
fn storage(e: EvidenceStorageError) -> Error {
    Error::with_source(K::Storage, e)
}
/// Inventory before PUT, verify outside SQL, then atomically publish. Retry exact proposal.
/// # Errors
/// Leaves unknown writes inventoried; never compensates by deleting or generating new IDs.
pub async fn publish_stream_segment(
    ledger: &impl StreamPublicationLedger,
    store: &impl StreamSegmentStorage,
    session: &StreamingSession,
    proposal: &StreamPublication,
    catalogs: AuthorityCatalogs<'_>,
    cancellation: &Cancellation,
    access: &impl StreamSegmentAccess,
) -> Result<StreamCaptureBoundary> {
    if let Some(receipt) = ledger
        .recover_publication(session, proposal, catalogs)
        .await?
    {
        return Ok(receipt);
    }
    ledger
        .stage(session, proposal, catalogs, cancellation)
        .await?;
    let stored = store
        .put_segment(proposal.segment(), access)
        .await
        .map_err(storage)?;
    if stored != proposal.segment().object() {
        return Err(invalid());
    }
    ledger
        .commit_segment(session, proposal, catalogs, cancellation)
        .await
}
/// Check durable denial both sides of bounded storage I/O; never return withdrawn bytes.
/// # Errors
/// Rejects unpublished/retired objects, current denial, expiry or corrupt storage.
pub async fn read_stream_segment(
    ledger: &impl StreamPublicationLedger,
    store: &impl StreamSegmentStorage,
    session: &StreamingSession,
    id: &EvidenceId,
    catalogs: AuthorityCatalogs<'_>,
    access: &impl StreamSegmentAccess,
) -> Result<StreamSegment> {
    let object = ledger.published_segment(session, id, catalogs).await?;
    let segment = store.get_segment(&object, access).await.map_err(storage)?;
    if segment.object() != object {
        return Err(invalid());
    }
    if ledger.published_segment(session, id, catalogs).await? != object {
        return Err(Error::new(K::Denied));
    }
    Ok(segment)
}

/// Retired exact object, kept only until physical deletion is confirmed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamCleanup {
    /// Opaque original publication intent.
    pub publication: EvidenceId,
    /// Exact scoped deletion descriptor, never read authority.
    pub object: StreamSegmentObject,
}
/// Independent storage-writer quiescence, distinct from provider connection quiescence.
pub trait StreamDeletionAuthority: Send + Sync {
    /// Authenticate exact cleanup and confirm no outstanding PUT can recreate it.
    /// # Errors
    /// Deny until independently confirmed; timeouts, cancellation and process loss do not suffice.
    fn confirm_storage_quiescent(
        &self,
        cleanup: &StreamCleanup,
    ) -> std::result::Result<(), EvidenceStorageError>;
}
/// Separate deletion capability; daily cleanup cannot enter the streaming namespace.
pub trait StreamDeletion: Send + Sync {
    /// Delete only after durable retirement and independent writer confirmation; repeat safely.
    fn delete_segment(
        &self,
        cleanup: &StreamCleanup,
        authority: &impl StreamDeletionAuthority,
    ) -> impl Future<Output = std::result::Result<(), EvidenceStorageError>> + Send;
}
