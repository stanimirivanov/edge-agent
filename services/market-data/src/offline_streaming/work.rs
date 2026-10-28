use super::*;
use edgeagent_contracts::{Component, MessageMetadata};
pub(super) struct Budget {
    used: std::sync::Mutex<(usize, usize)>,
    limits: OfflineStreamLimits,
}
impl Budget {
    pub fn new(rows: usize, bytes: usize, limits: OfflineStreamLimits) -> Self {
        Self {
            used: std::sync::Mutex::new((rows, bytes)),
            limits,
        }
    }
    fn reserve(&self, rows: usize, bytes: usize) -> Result<()> {
        let mut used = self
            .used
            .lock()
            .map_err(|_| OfflineStreamError::Unconfirmed)?;
        let rows = used
            .0
            .checked_add(rows)
            .ok_or(OfflineStreamError::Overload)?;
        let bytes = used
            .1
            .checked_add(bytes)
            .ok_or(OfflineStreamError::Overload)?;
        if rows > usize::from(self.limits.history.get())
            || bytes > self.limits.history_bytes.get() as usize
        {
            return Err(OfflineStreamError::Overload);
        }
        *used = (rows, bytes);
        Ok(())
    }
}
pub(super) async fn inputs<C: OfflineStreamClock>(
    ledger: &PostgresStreamingLedger,
    s: &OfflineStreamSource<'_>,
    clock: &C,
    cancel: &Cancellation,
) -> Result<()> {
    s.inputs
        .recheck(s.session, s.catalogs, clock.now(), cancel)
        .map_err(|_| OfflineStreamError::Authority)?;
    let derived = s
        .profile
        .owner_inputs(s.session, s.selection, s.inputs.expires_at())
        .map_err(|_| OfflineStreamError::Authority)?;
    if derived != *s.inputs {
        return Err(OfflineStreamError::Authority);
    }
    ledger
        .load_stream_inputs(s.session, s.inputs, s.catalogs, cancel)
        .await
        .map_err(|_| OfflineStreamError::Authority)?;
    Ok(())
}
pub(super) struct Access<'a, 's, C> {
    pub source: &'a OfflineStreamSource<'s>,
    pub clock: &'a C,
    pub cancellation: &'a Cancellation,
}
impl<C: OfflineStreamClock> StreamSegmentAccess for Access<'_, '_, C> {
    fn now(&self) -> UtcInstant {
        self.clock.now()
    }
    fn authorize(&self, o: &StreamSegmentObject) -> std::result::Result<(), EvidenceStorageError> {
        let deny = || EvidenceStorageError::new(EvidenceStorageErrorKind::Denied);
        let s = self.source;
        let at = self.clock.now();
        s.inputs
            .recheck(s.session, s.catalogs, at, self.cancellation)
            .map_err(|_| deny())?;
        let scope = s
            .session
            .data()
            .scopes
            .iter()
            .find(|v| format!("{:032x}", v.instrument.get()) == o.scope.instrument.as_str())
            .ok_or_else(deny)?;
        let rights = s
            .catalogs
            .rights
            .at(at)
            .find(|r| r.value().scope == *scope)
            .ok_or_else(deny)?;
        let hex = |v: u128| format!("{v:032x}");
        if o.scope.session != s.session.data().id
            || o.scope.session_sha256 != DailyEvidenceArtifact::hash(s.session.bytes())
            || o.scope.principal.as_str() != hex(scope.principal.get())
            || o.scope.account.as_str() != hex(scope.account.get())
            || o.scope.source.as_str() != hex(scope.source.get())
            || o.scope.endpoint.as_str() != hex(scope.endpoint.get())
            || o.scope.audience.as_str() != hex(scope.audience.get())
            || o.scope.storage.as_str() != hex(scope.storage.get())
            || o.scope.rights.id.as_str() != hex(rights.id().get())
            || o.scope.rights.revision.as_str() != rights.revision().get().to_string()
            || i128::from(o.delete_by.seconds) * 1000 + i128::from(o.delete_by.nanos / 1_000_000)
                > i128::from(s.inputs.expires_at().unix_millis())
        {
            return Err(deny());
        }
        Ok(())
    }
}
pub(super) struct Work {
    pub index: usize,
    pub dispatch: StreamDispatch,
    pub accepted: Vec<IexAcceptedObservation>,
    pub tracker: ContinuityTracker,
    pub heads: BTreeMap<InstrumentId, EvidenceDigest>,
    pub published: BTreeMap<InstrumentId, Published>,
    pub connection: StreamingConnection,
    pub restore: StreamRestore,
}
pub(super) struct Published {
    pub id: EvidenceId,
    pub canonical: StreamSegmentObject,
    pub native: StreamCompanionObject,
}
pub(super) struct Done {
    pub work: Work,
    pub result: Result<()>,
}
#[allow(
    clippy::too_many_arguments,
    reason = "Bounded explicit existing runtime/authority ports."
)]
pub(super) async fn execute<S: StreamSegmentStorage, C: OfflineStreamClock>(
    mut w: Work,
    s: &OfflineStreamSource<'_>,
    ledger: &PostgresStreamingLedger,
    store: &S,
    clock: &C,
    cancel: &Cancellation,
    limits: OfflineStreamLimits,
    budget: &Budget,
) -> Done {
    let operation = async {
        let connection = &w.connection;
        ledger
            .check_stream_restored(s.session, &w.restore, s.catalogs)
            .await
            .map_err(|_| OfflineStreamError::Authority)?;
        inputs(ledger, s, clock, cancel).await?;
        for a in &w.accepted {
            w.tracker
                .ingest(
                    s.epoch,
                    PreciseInstant::from_millis(clock.now()),
                    a.canonical().clone(),
                )
                .map_err(|_| OfflineStreamError::Continuity)?;
        }
        if w.tracker.status().0 != ContinuityState::Tracking {
            return Err(OfflineStreamError::Continuity);
        }
        let mut groups: BTreeMap<InstrumentId, Vec<&IexAcceptedObservation>> = BTreeMap::new();
        for a in &w.accepted {
            groups
                .entry(a.canonical().context().origin().0)
                .or_default()
                .push(a);
        }
        let mut proposals = Vec::with_capacity(groups.len());
        let mut bytes = 0usize;
        for (instrument, accepted) in groups {
            let first = accepted
                .iter()
                .map(|a| a.native().frames[0])
                .min()
                .ok_or(OfflineStreamError::Configuration)?;
            let end = accepted
                .iter()
                .map(|a| a.native().frames[1])
                .max()
                .ok_or(OfflineStreamError::Configuration)?;
            let records = accepted
                .iter()
                .map(|a| CapturedStreamObservation {
                    frame: a.native().frames[1],
                    item: a.native().item,
                    observation: a.canonical(),
                })
                .collect::<Vec<_>>();
            let at = clock.now();
            let segment = freeze_stream_segment(
                s.session,
                connection,
                s.catalogs,
                at,
                cancel,
                StreamSegmentInput {
                    instrument,
                    epoch: s.epoch,
                    frames: [first, end],
                    previous: w.heads.get(&instrument),
                    limits: s.segment_limits,
                    delete_by: s.delete_by,
                    records: &records,
                },
            )
            .map_err(|_| OfflineStreamError::Semantics)?;
            let native = s
                .profile
                .companion(&segment, s.inputs, &accepted, s.native_limits)
                .map_err(|_| OfflineStreamError::Semantics)?;
            let key = serde_json::to_vec(&(
                s.session.data().id.as_str(),
                connection.generation(),
                first,
                end,
                instrument.get(),
            ))
            .map_err(|_| OfflineStreamError::Configuration)?;
            let digest = DailyEvidenceArtifact::hash(&key);
            let id = EvidenceId::new(
                digest
                    .as_str()
                    .get(..32)
                    .ok_or(OfflineStreamError::Configuration)?,
            )
            .map_err(|_| OfflineStreamError::Configuration)?;
            let time = chrono::DateTime::from_timestamp_millis(at.unix_millis())
                .ok_or(OfflineStreamError::Configuration)?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let p = StreamPublication::new(
                id.clone(),
                segment,
                MessageMetadata {
                    id: format!("stream-publication-{}", id.as_str()),
                    source: Component::MarketData.source_uri().into(),
                    message_type: STREAM_PUBLISHED.message_type().into(),
                    subject: format!("stream/{}", s.session.data().id.as_str()),
                    time,
                    data_schema: STREAM_PUBLISHED.data_schema().into(),
                    correlation_id: id.as_str().into(),
                    causation_id: s.session.data().id.as_str().into(),
                    idempotency_key: id.as_str().into(),
                    partition_key: format!("stream/{}", s.session.data().id.as_str()),
                    trace_parent: format!(
                        "00-{}-{}-01",
                        id.as_str(),
                        digest
                            .as_str()
                            .get(32..48)
                            .ok_or(OfflineStreamError::Configuration)?
                    ),
                    trace_state: None,
                },
            )
            .map_err(|_| OfflineStreamError::Configuration)?
            .with_companion(native)
            .map_err(|_| OfflineStreamError::Semantics)?;
            bytes = bytes
                .checked_add(
                    p.bytes()
                        .map_err(|_| OfflineStreamError::Configuration)?
                        .len(),
                )
                .ok_or(OfflineStreamError::Configuration)?;
            if bytes > limits.history_bytes.get() as usize {
                return Err(OfflineStreamError::Overload);
            }
            proposals.push((instrument, p));
        }
        // Count uncertain reservations too. A successful run must remain reconstructible under
        // its own history ceiling; operational failure never refunds this in-memory admission.
        budget.reserve(proposals.len(), bytes)?;
        // Inventory every instrument before any PUT: partial I/O always has exact recoverable intents.
        for (_, p) in &proposals {
            ledger
                .stage(s.session, p, s.catalogs, cancel)
                .await
                .map_err(|_| OfflineStreamError::Uncertain)?;
        }
        let access = Access {
            source: s,
            clock,
            cancellation: cancel,
        };
        for (instrument, p) in &proposals {
            publish_stream_segment(ledger, store, s.session, p, s.catalogs, cancel, &access)
                .await
                .map_err(|_| OfflineStreamError::Uncertain)?;
            w.heads.insert(*instrument, p.segment().digest().clone());
            w.published.insert(
                *instrument,
                Published {
                    id: p.id().clone(),
                    canonical: p.segment().object(),
                    native: p
                        .companion()
                        .ok_or(OfflineStreamError::Configuration)?
                        .object(),
                },
            );
        }
        inputs(ledger, s, clock, cancel).await?;
        Ok(())
    };
    let result = tokio::time::timeout(
        Duration::from_millis(u64::from(limits.io_ms.get())),
        operation,
    )
    .await
    .unwrap_or(Err(OfflineStreamError::Deadline));
    Done { work: w, result }
}
