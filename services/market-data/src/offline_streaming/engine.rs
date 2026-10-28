use super::*;
use futures_util::{StreamExt, stream::FuturesUnordered};
fn stopped(error: OfflineStreamError) -> Option<StreamStop> {
    match error {
        OfflineStreamError::Overload => Some(StreamStop::Overload),
        OfflineStreamError::Semantics => Some(StreamStop::Binding),
        OfflineStreamError::Continuity => Some(StreamStop::SourceFailure),
        _ => None,
    }
}
async fn mark(
    ledger: &PostgresStreamingLedger,
    s: &OfflineStreamSource<'_>,
    restore: &StreamRestore,
    error: OfflineStreamError,
    io: Duration,
) -> Result<()> {
    let operation = async {
        if let Some(reason) = stopped(error) {
            ledger
                .record_stream_incident(s.session, restore, reason, s.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Unconfirmed)?;
        } else {
            // Operational uncertainty remains recoverable, but this worker's old witness is fenced.
            ledger
                .begin_restore(s.session, s.catalogs)
                .await
                .map_err(|_| OfflineStreamError::Unconfirmed)?;
        }
        Ok(())
    };
    tokio::time::timeout(io, operation)
        .await
        .map_err(|_| OfflineStreamError::Unconfirmed)?
}
impl<S: StreamSegmentStorage, C: OfflineStreamClock> OfflineStreamRuntime<'_, S, C> {
    /// Consume one explicit finite script per source. At most `inflight` publication futures
    /// coexist; no task is detached, retried, reconnected or allowed to release capacity.
    /// # Errors
    /// Preserves uncertain preparations; fatal semantics latch incidents, other failures fence
    /// fresh blocked restores. An unapproved gate consumes no script or account budget.
    pub async fn run(&mut self, mut frames: Vec<VecDeque<OfflineStreamFrame>>) -> Result<()> {
        if self.phase != Phase::AwaitApproval || frames.len() != self.sources.len() {
            return Err(OfflineStreamError::Configuration);
        }
        if self.cancellation.is_cancelled() {
            return Err(OfflineStreamError::Cancelled);
        }
        let count = frames.iter().map(VecDeque::len).sum::<usize>();
        let records = frames
            .iter()
            .flat_map(|q| q.iter())
            .try_fold(
                self.lanes.iter().map(|l| l.native.len()).sum::<usize>(),
                |n, f| n.checked_add(f.records.len()),
            )
            .ok_or(OfflineStreamError::Configuration)?;
        if count > usize::from(self.limits.frames.get())
            || records > usize::from(self.limits.records.get())
            || frames
                .iter()
                .flat_map(|q| q.iter())
                .any(|f| f.records.len() > 1024)
        {
            return Err(OfflineStreamError::Configuration);
        }
        let deadline = tokio::time::Instant::now()
            + Duration::from_millis(u64::from(self.limits.run_ms.get()));
        let gates = async {
            for i in 0..self.sources.len() {
                self.gate(i).await?;
            }
            Ok(())
        };
        tokio::time::timeout_at(deadline, gates)
            .await
            .map_err(|_| OfflineStreamError::Deadline)??;
        // Consuming the attempt before the first mutation also fences local reuse after
        // the caller drops this future. Recovery requires a new durable restore revision.
        self.phase = Phase::Executing;
        let result = tokio::time::timeout_at(deadline, self.run_inner(&mut frames))
            .await
            .unwrap_or(Err(OfflineStreamError::Deadline));
        if result.is_ok() {
            self.phase = Phase::Complete;
            return Ok(());
        }
        self.phase = Phase::Failed;
        // No asynchronous Drop or retry. One bounded fencing attempt per otherwise active lane.
        let unconfirmed = self.lanes.iter().any(|l| l.failed && !l.fenced);
        let fencing = async {
            for (s, l) in self
                .sources
                .iter()
                .zip(&self.lanes)
                .filter(|(_, l)| !l.failed)
            {
                if mark(
                    &self.ledger,
                    s,
                    &l.restore,
                    OfflineStreamError::Uncertain,
                    self.io(),
                )
                .await
                .is_err()
                {
                    return Err(OfflineStreamError::Unconfirmed);
                }
            }
            if unconfirmed {
                Err(OfflineStreamError::Unconfirmed)
            } else {
                Ok(())
            }
        };
        if !matches!(tokio::time::timeout(self.io(), fencing).await, Ok(Ok(()))) {
            return Err(OfflineStreamError::Unconfirmed);
        }
        result
    }
    async fn run_inner(&mut self, scripts: &mut [VecDeque<OfflineStreamFrame>]) -> Result<()> {
        let io = self.io();
        let limits = self.limits;
        let ledgers = self
            .lanes
            .iter()
            .map(|l| self.ledger.fenced(&l.restore, StreamRuntimePhase::Running))
            .collect::<Vec<_>>();
        for (i, ((s, l), ledger)) in self
            .sources
            .iter()
            .zip(&mut self.lanes)
            .zip(&ledgers)
            .enumerate()
        {
            history::arm(s, l, ledger, self.clock).await?;
            if scripts.get(i).is_none_or(VecDeque::is_empty) {
                continue;
            }
            l.connection = Some(
                tokio::time::timeout(
                    io,
                    ledger.reserve(s.session, s.catalogs, &self.cancellation),
                )
                .await
                .map_err(|_| OfflineStreamError::Deadline)?
                .map_err(|_| OfflineStreamError::Uncertain)?,
            );
        }
        let mut flights = FuturesUnordered::new();
        let mut failure = None;
        loop {
            if self.cancellation.is_cancelled() {
                return Err(OfflineStreamError::Cancelled);
            }
            let mut progressed = false;
            for (i, ((s, l), script)) in self
                .sources
                .iter()
                .zip(&mut self.lanes)
                .zip(scripts.iter_mut())
                .enumerate()
            {
                if l.failed || l.tracker.is_none() || !l.queued.is_empty() {
                    continue;
                }
                let Some(frame) = script.pop_front() else {
                    continue;
                };
                progressed = true;
                let operation = async {
                    let connection = l
                        .connection
                        .as_ref()
                        .ok_or(OfflineStreamError::Configuration)?;
                    if connection.usage().frames.checked_add(1) != Some(frame.arrival.ordinal) {
                        return Err(OfflineStreamError::Semantics);
                    }
                    ledgers[i]
                        .check_stream_restored(s.session, &l.restore, s.catalogs)
                        .await
                        .map_err(|_| OfflineStreamError::Authority)?;
                    work::inputs(&ledgers[i], s, self.clock, &self.cancellation).await?;
                    let charged = ledgers[i]
                        .charge(
                            s.session,
                            connection,
                            s.catalogs,
                            &self.cancellation,
                            frame.arrival,
                        )
                        .await
                        .map_err(|_| OfflineStreamError::Uncertain)?;
                    if charged.repeated || !charged.within_limits {
                        return Err(OfflineStreamError::Uncertain);
                    }
                    l.connection = Some(
                        StreamingConnection::from_committed(
                            s.session,
                            connection.generation(),
                            charged.usage,
                        )
                        .map_err(|_| OfflineStreamError::Uncertain)?,
                    );
                    if frame.activity {
                        l.tracker
                            .as_mut()
                            .ok_or(OfflineStreamError::Configuration)?
                            .heartbeat(s.epoch, PreciseInstant::from_millis(self.clock.now()), None)
                            .map_err(|_| OfflineStreamError::Continuity)?;
                    }
                    let mut accepted = Vec::with_capacity(frame.records.len());
                    let mut item = None;
                    for input in frame.records {
                        if input.frames[1] != frame.arrival.ordinal
                            || item.is_some_and(|old| old >= input.item)
                        {
                            return Err(OfflineStreamError::Semantics);
                        }
                        item = Some(input.item);
                        accepted.push(
                            s.profile
                                .accept(
                                    s.session,
                                    s.selection,
                                    s.catalogs,
                                    s.references(),
                                    self.clock.now(),
                                    &self.cancellation,
                                    input,
                                )
                                .map_err(|_| OfflineStreamError::Semantics)?,
                        );
                    }
                    for a in &accepted {
                        self.supervisor
                            .enqueue(s.scope.binding().0, s.epoch, a.canonical().clone())
                            .map_err(|e| match e.reason() {
                                SupervisionError::Stopped(StreamStop::Overload) => {
                                    OfflineStreamError::Overload
                                }
                                SupervisionError::Stopped(StreamStop::Cancelled) => {
                                    OfflineStreamError::Cancelled
                                }
                                _ => OfflineStreamError::Semantics,
                            })?;
                    }
                    if !accepted.is_empty() {
                        l.queued.push_back(accepted);
                    }
                    Ok(())
                };
                let result = tokio::time::timeout(io, operation)
                    .await
                    .unwrap_or(Err(OfflineStreamError::Deadline));
                if let Err(error) = result {
                    l.failed = true;
                    self.supervisor
                        .stop_source(
                            s.scope.binding().0,
                            stopped(error).unwrap_or(StreamStop::SourceFailure),
                        )
                        .map_err(|_| OfflineStreamError::Configuration)?;
                    let recorded = mark(&self.ledger, s, &l.restore, error, io).await;
                    l.fenced = recorded.is_ok();
                    failure.get_or_insert(if recorded.is_err() {
                        OfflineStreamError::Unconfirmed
                    } else {
                        error
                    });
                }
            }
            while flights.len() < usize::from(limits.inflight.get()) {
                let Some(dispatch) = self
                    .supervisor
                    .dispatch_next()
                    .map_err(|_| OfflineStreamError::Cancelled)?
                else {
                    break;
                };
                let i = self
                    .sources
                    .iter()
                    .position(|s| s.scope.binding().0 == dispatch.identity().0)
                    .ok_or(OfflineStreamError::Configuration)?;
                let lane = self
                    .lanes
                    .get_mut(i)
                    .ok_or(OfflineStreamError::Configuration)?;
                let accepted = lane
                    .queued
                    .pop_front()
                    .ok_or(OfflineStreamError::Configuration)?;
                let count = NonZeroU16::new(
                    u16::try_from(accepted.len()).map_err(|_| OfflineStreamError::Configuration)?,
                )
                .ok_or(OfflineStreamError::Configuration)?;
                let dispatch = self
                    .supervisor
                    .extend_dispatch(dispatch, count)
                    .map_err(|_| OfflineStreamError::Overload)?;
                let w = work::Work {
                    index: i,
                    dispatch,
                    accepted,
                    tracker: lane
                        .tracker
                        .take()
                        .ok_or(OfflineStreamError::Configuration)?,
                    heads: std::mem::take(&mut lane.heads),
                    published: BTreeMap::new(),
                    connection: lane
                        .connection
                        .take()
                        .ok_or(OfflineStreamError::Configuration)?,
                    restore: StreamRestore::from_committed(
                        lane.restore.identity().0.clone(),
                        lane.restore.identity().1,
                    )
                    .map_err(|_| OfflineStreamError::Configuration)?,
                };
                flights.push(work::execute(
                    w,
                    &self.sources[i],
                    &ledgers[i],
                    self.store,
                    self.clock,
                    &self.cancellation,
                    limits,
                    &self.budget,
                ));
            }
            if let Some(done) = flights.next().await {
                let work::Work {
                    index,
                    dispatch,
                    accepted,
                    tracker,
                    heads,
                    connection,
                    published,
                    ..
                } = done.work;
                let l = self
                    .lanes
                    .get_mut(index)
                    .ok_or(OfflineStreamError::Configuration)?;
                l.tracker = Some(tracker);
                l.heads = heads;
                l.connection = Some(connection);
                let outcome = done.result.and_then(|()| {
                    self.supervisor
                        .confirm(dispatch)
                        .map_err(|_| OfflineStreamError::Cancelled)
                });
                match outcome {
                    Ok(()) => {
                        for observation in accepted {
                            let publication = published
                                .get(&observation.canonical().context().origin().0)
                                .ok_or(OfflineStreamError::Configuration)?;
                            l.native.push(Retained {
                                observation,
                                publication: publication.id.clone(),
                                canonical: publication.canonical.clone(),
                                native: publication.native.clone(),
                            });
                        }
                    }
                    Err(error) => {
                        l.failed = true;
                        let s = &self.sources[index];
                        self.supervisor
                            .stop_source(
                                s.scope.binding().0,
                                stopped(error).unwrap_or(StreamStop::SourceFailure),
                            )
                            .map_err(|_| OfflineStreamError::Configuration)?;
                        let recorded = mark(&self.ledger, s, &l.restore, error, io).await;
                        l.fenced = recorded.is_ok();
                        failure.get_or_insert(if recorded.is_err() {
                            OfflineStreamError::Unconfirmed
                        } else {
                            error
                        });
                    }
                }
            } else if !progressed {
                break;
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.supervisor
            .check_health()
            .map_err(|_| OfflineStreamError::NotReady)?;
        for i in 0..self.sources.len() {
            self.gate(i).await?;
        }
        Ok(())
    }
}
