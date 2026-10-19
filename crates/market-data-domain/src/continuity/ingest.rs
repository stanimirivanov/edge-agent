use super::{ContinuityError, ContinuityMode, ContinuityState, ContinuityTracker, IngestOutcome};
use crate::{
    CanonicalObservation, FeedEpoch, InstrumentId, ObservationId, ObservationRevision,
    ObservationValue, PreciseInstant,
};

impl ContinuityTracker {
    /// Retain one source revision, repair a bounded prefix, or classify exact overlap.
    /// Duplicate comparison excludes only local receipt; it never refreshes the
    /// original market/receipt timestamps. Known incidents require explicit completion.
    ///
    /// # Errors
    /// Returns typed scope/clock/epoch, budget, identity, predecessor or overlap failures.
    /// Conflicts halt the scope; capacity/sequence overflow exhaust it without eviction.
    pub fn ingest(
        &mut self,
        epoch: FeedEpoch,
        at: PreciseInstant,
        record: CanonicalObservation,
    ) -> Result<IngestOutcome, ContinuityError> {
        self.epoch(epoch)?;
        let (_, provider, transformation) = record.context().origin();
        if provider != self.scope.provider
            || transformation != self.scope.transformation
            || record.context().session_conditions().0 != self.scope.session
        {
            return Err(ContinuityError::Scope);
        }
        if record.times().received_at() > at {
            return Err(ContinuityError::Clock);
        }
        self.advance_clock(at)?;
        match self.state {
            ContinuityState::Halted => return Err(ContinuityError::Halted),
            ContinuityState::Exhausted => return Err(ContinuityError::Exhausted),
            _ => {}
        }
        if let Some(active) = &mut self.active {
            if active.rows >= self.policy.capacity {
                self.active = None;
                return Err(ContinuityError::RecoveryWork);
            }
            active.rows += 1;
        }
        let (id, revision, _) = record.version().lineage();
        if let Some(existing) = self
            .versions
            .get(&(id, revision))
            .and_then(|key| self.records.get(key))
        {
            if same_revision(existing, &record) {
                return Ok(IngestOutcome::Duplicate);
            }
            return self.halt(ContinuityError::Conflict);
        }
        if self.records.len() >= self.policy.capacity {
            self.state = ContinuityState::Exhausted;
            self.active = None;
            return Err(ContinuityError::Capacity);
        }
        let key = match self.mode {
            ContinuityMode::Snapshots => {
                u64::try_from(self.records.len()).map_err(|_| ContinuityError::Capacity)?
            }
            ContinuityMode::Contiguous { next } => {
                let sequence = record
                    .sequence()
                    .ok_or(ContinuityError::ContinuityUnknown)?;
                if sequence < next || self.records.contains_key(&sequence) {
                    return self.halt(ContinuityError::Conflict);
                }
                sequence
            }
        };
        if self.mode == ContinuityMode::Snapshots
            && let Err(error) = self.validate_record(&record)
        {
            return self.halt(error);
        }
        self.records.insert(key, record);
        self.versions.insert((id, revision), key);
        let released = match self.mode {
            ContinuityMode::Snapshots => {
                self.current.insert(id, key);
                1
            }
            ContinuityMode::Contiguous { .. } => self.advance_prefix()?,
        };
        if self.missing_range().is_some() && self.state == ContinuityState::Tracking {
            self.state = ContinuityState::Recovering;
        }
        if released == 0 {
            Ok(IngestOutcome::Buffered)
        } else {
            Ok(IngestOutcome::Accepted { released })
        }
    }
    fn advance_prefix(&mut self) -> Result<usize, ContinuityError> {
        let mut released = 0;
        while let ContinuityMode::Contiguous { next } = self.mode {
            let Some(record) = self.records.get(&next) else {
                break;
            };
            let Some(successor) = next.checked_add(1) else {
                self.state = ContinuityState::Exhausted;
                self.active = None;
                return Err(ContinuityError::SequenceOverflow);
            };
            if let Err(error) = self.validate_record(record) {
                return self.halt(error);
            }
            let id = record.version().lineage().0;
            self.current.insert(id, next);
            self.mode = ContinuityMode::Contiguous { next: successor };
            released += 1;
        }
        Ok(released)
    }
    fn halt<T>(&mut self, error: ContinuityError) -> Result<T, ContinuityError> {
        self.state = ContinuityState::Halted;
        self.active = None;
        Err(error)
    }
    fn validate_record(&self, record: &CanonicalObservation) -> Result<(), ContinuityError> {
        let (id, _, predecessor) = record.version().lineage();
        let current = self.current.get(&id).and_then(|key| self.records.get(key));
        match (predecessor, current) {
            (None, None) => {}
            (Some(previous), Some(current))
                if current.version().lineage().1 == previous
                    && current.context().origin().0 == record.context().origin().0 => {}
            _ => return Err(ContinuityError::Predecessor),
        }
        if let Some(ObservationValue::Bar(bar)) = record.value() {
            let (start, end) = bar.bucket().0.bounds();
            for other in self
                .current
                .iter()
                .filter(|(other_id, _)| **other_id != id)
                .filter_map(|(_, key)| self.records.get(key))
            {
                if other.context().origin().0 != record.context().origin().0 {
                    continue;
                }
                if let Some(ObservationValue::Bar(other)) = other.value() {
                    let (other_start, other_end) = other.bucket().0.bounds();
                    if start < other_end && other_start < end {
                        return Err(ContinuityError::Overlap);
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn selected(
        &self,
        id: ObservationId,
        revision: ObservationRevision,
    ) -> Result<&CanonicalObservation, ContinuityError> {
        let key = self
            .versions
            .get(&(id, revision))
            .ok_or(ContinuityError::Input)?;
        let record = self.records.get(key).ok_or(ContinuityError::Input)?;
        if let ContinuityMode::Contiguous { next } = self.mode
            && *key >= next
        {
            return Err(ContinuityError::Input);
        }
        if self.current.get(&id) != Some(key) || record.value().is_none() {
            return Err(ContinuityError::Superseded);
        }
        Ok(record)
    }
    pub(crate) fn instrument(record: &CanonicalObservation) -> InstrumentId {
        record.context().origin().0
    }
}
fn same_revision(a: &CanonicalObservation, b: &CanonicalObservation) -> bool {
    a.context() == b.context()
        && a.version() == b.version()
        && a.sequence() == b.sequence()
        && a.value() == b.value()
        && a.times().event_at() == b.times().event_at()
        && a.times().provider_times() == b.times().provider_times()
}
