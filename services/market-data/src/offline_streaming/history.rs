use super::*;
pub(super) async fn restore<S: StreamSegmentStorage, C: OfflineStreamClock>(
    runtime: &OfflineStreamRuntime<'_, S, C>,
    i: usize,
    restore: StreamRestore,
    rows: &mut usize,
    bytes: &mut usize,
    records: &mut usize,
) -> Result<Lane> {
    let source = runtime
        .sources
        .get(i)
        .ok_or(OfflineStreamError::Configuration)?;
    let ledger = runtime
        .ledger
        .fenced(&restore, StreamRuntimePhase::Restoring);
    work::inputs(&ledger, source, runtime.clock, &runtime.cancellation).await?;
    let metadata = ledger
        .stream_history(
            source.session,
            source.catalogs,
            runtime.limits.history.get(),
        )
        .await
        .map_err(|_| OfflineStreamError::History)?;
    *rows = rows
        .checked_add(metadata.len())
        .ok_or(OfflineStreamError::History)?;
    for entry in &metadata {
        *bytes = bytes
            .checked_add(entry.preparation_bytes)
            .ok_or(OfflineStreamError::History)?;
        if entry.retired || entry.preparation_bytes == 0 {
            return Err(OfflineStreamError::History);
        }
    }
    if *rows > usize::from(runtime.limits.history.get())
        || *bytes > runtime.limits.history_bytes.get() as usize
    {
        return Err(OfflineStreamError::History);
    }
    // Inspect sizes before fetching any private body. Restoring fences admit only pre-existing intents.
    for entry in &metadata {
        let p = ledger
            .recover_segment(source.session, &entry.publication, source.catalogs)
            .await
            .map_err(|_| OfflineStreamError::History)?
            .ok_or(OfflineStreamError::History)?;
        if p.companion().is_none()
            || p.bytes().map_err(|_| OfflineStreamError::History)?.len() != entry.preparation_bytes
        {
            return Err(OfflineStreamError::History);
        }
    }
    let access = work::Access {
        source,
        clock: runtime.clock,
        cancellation: &runtime.cancellation,
    };
    for scope in &source.session.data().scopes {
        recover_pending_stream_segment(
            &ledger,
            &ledger,
            runtime.store,
            source.session,
            &restore,
            scope.instrument,
            source.catalogs,
            &runtime.cancellation,
            &access,
        )
        .await
        .map_err(|_| OfflineStreamError::Uncertain)?;
    }
    let mut ordered = Vec::with_capacity(metadata.len());
    for entry in &metadata {
        let p = ledger
            .recover_segment(source.session, &entry.publication, source.catalogs)
            .await
            .map_err(|_| OfflineStreamError::History)?
            .ok_or(OfflineStreamError::History)?;
        if ledger
            .recover_publication(source.session, &p, source.catalogs)
            .await
            .map_err(|_| OfflineStreamError::History)?
            .is_none()
        {
            return Err(OfflineStreamError::History);
        }
        let end = p.segment().data().frames[1]
            .as_str()
            .parse::<u64>()
            .map_err(|_| OfflineStreamError::History)?;
        ordered.push((end, p));
    }
    ordered.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            a.1.segment()
                .data()
                .scope
                .instrument
                .cmp(&b.1.segment().data().scope.instrument)
        })
    });
    let mut tracker = ContinuityTracker::new(
        source.scope,
        source.epoch,
        ContinuityMode::Snapshots,
        source.continuity,
        PreciseInstant::from_millis(runtime.clock.now()),
    )
    .map_err(|_| OfflineStreamError::Configuration)?;
    let mut heads = BTreeMap::new();
    let mut native = Vec::new();
    for (_, p) in ordered {
        ledger
            .check_restoring(source.session, &restore, source.catalogs)
            .await
            .map_err(|_| OfflineStreamError::Authority)?;
        let (segment, companion) = read_stream_publication(
            &ledger,
            runtime.store,
            source.session,
            p.id(),
            source.catalogs,
            &access,
        )
        .await
        .map_err(|_| OfflineStreamError::History)?;
        let companion = companion.ok_or(OfflineStreamError::History)?;
        if segment.data().epoch.as_str() != source.epoch.get().to_string() {
            return Err(OfflineStreamError::History);
        }
        if companion.data().owner_inputs != source.inputs.bytes() {
            return Err(OfflineStreamError::Authority);
        }
        let instrument = InstrumentId::new(
            u128::from_str_radix(segment.data().scope.instrument.as_str(), 16)
                .map_err(|_| OfflineStreamError::History)?,
        )
        .map_err(|_| OfflineStreamError::History)?;
        if segment.data().previous.as_ref() != heads.get(&instrument) {
            return Err(OfflineStreamError::History);
        }
        *records = records
            .checked_add(segment.data().records.len())
            .ok_or(OfflineStreamError::History)?;
        if *records > usize::from(runtime.limits.records.get()) {
            return Err(OfflineStreamError::History);
        }
        let values = source
            .profile
            .replay_companion(
                IexReplayContext {
                    session: source.session,
                    selection: source.selection,
                    catalogs: source.catalogs,
                    references: source.references(),
                    at: runtime.clock.now(),
                    cancellation: &runtime.cancellation,
                },
                &segment,
                &companion,
            )
            .map_err(|_| OfflineStreamError::Semantics)?;
        for value in &values {
            tracker
                .ingest(
                    source.epoch,
                    PreciseInstant::from_millis(runtime.clock.now()),
                    value.canonical().clone(),
                )
                .map_err(|_| OfflineStreamError::Continuity)?;
        }
        native.extend(values.into_iter().map(|observation| Retained {
            observation,
            publication: p.id().clone(),
            canonical: segment.object(),
            native: companion.object(),
        }));
        heads.insert(instrument, segment.digest().clone());
        ledger
            .check_restoring(source.session, &restore, source.catalogs)
            .await
            .map_err(|_| OfflineStreamError::Authority)?;
    }
    Ok(Lane {
        restore,
        tracker: Some(tracker),
        heads,
        native,
        queued: VecDeque::new(),
        connection: None,
        failed: false,
        fenced: false,
    })
}
pub(super) async fn arm<C: OfflineStreamClock>(
    s: &OfflineStreamSource<'_>,
    lane: &mut Lane,
    ledger: &PostgresStreamingLedger,
    clock: &C,
) -> Result<()> {
    // There was no active feed during independent approval. Start its local activity clock
    // now, replaying original immutable values without changing any market/receipt times.
    let mut tracker = ContinuityTracker::new(
        s.scope,
        s.epoch,
        ContinuityMode::Snapshots,
        s.continuity,
        PreciseInstant::from_millis(clock.now()),
    )
    .map_err(|_| OfflineStreamError::Configuration)?;
    let mut checked = std::collections::BTreeSet::new();
    for retained in &lane.native {
        if checked.insert(retained.publication.clone()) {
            let object = ledger
                .published_segment(s.session, &retained.publication, s.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Authority)?;
            let native = ledger
                .published_companion(s.session, &retained.publication, s.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Authority)?
                .ok_or(OfflineStreamError::History)?;
            if object != retained.canonical || native != retained.native {
                return Err(OfflineStreamError::History);
            }
        }
        tracker
            .ingest(
                s.epoch,
                PreciseInstant::from_millis(clock.now()),
                retained.observation.canonical().clone(),
            )
            .map_err(|_| OfflineStreamError::Continuity)?;
    }
    lane.tracker = Some(tracker);
    Ok(())
}
