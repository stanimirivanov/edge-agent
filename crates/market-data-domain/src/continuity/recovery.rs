use super::{
    ActiveRecovery, ContinuityError, ContinuityMode, ContinuityState, ContinuityTracker,
    RecoveryTicket, elapsed,
};
use crate::{CanonicalObservation, FeedEpoch, PreciseInstant};

impl ContinuityTracker {
    /// Record explicit source activity, optionally announcing the last source
    /// sequence. A heartbeat never fills a hole or clears an incident. Ingesting
    /// historical/replayed data alone is not a live heartbeat.
    ///
    /// # Errors
    /// Returns `Epoch`, `Clock` or `Watermark`; snapshot scopes reject sequence claims.
    pub fn heartbeat(
        &mut self,
        epoch: FeedEpoch,
        at: PreciseInstant,
        watermark: Option<u64>,
    ) -> Result<(), ContinuityError> {
        self.epoch(epoch)?;
        if let (ContinuityMode::Contiguous { next }, Some(mark)) = (self.mode, watermark)
            && !self.current.is_empty()
            && mark < next.saturating_sub(1)
        {
            return Err(ContinuityError::Watermark);
        }
        if (self.mode == ContinuityMode::Snapshots && watermark.is_some())
            || watermark
                .zip(self.watermark)
                .is_some_and(|(new, old)| new < old)
        {
            return Err(ContinuityError::Watermark);
        }
        self.advance_clock(at)?;
        self.heartbeat = Some(at);
        if let Some(mark) = watermark {
            self.watermark = Some(mark);
        }
        if self.missing_range().is_some() && self.state == ContinuityState::Tracking {
            self.state = ContinuityState::Recovering;
        }
        Ok(())
    }
    /// Inclusive bounded recovery window, never a materialized unbounded range.
    /// Returns no range for snapshots, or when the known prefix is complete.
    #[must_use]
    pub fn missing_range(&self) -> Option<(u64, u64)> {
        let ContinuityMode::Contiguous { next } = self.mode else {
            return None;
        };
        let highest = self
            .records
            .last_key_value()
            .map(|(key, _)| *key)
            .into_iter()
            .chain(self.watermark)
            .max()?;
        if highest < next {
            return None;
        }
        let width = u64::try_from(self.policy.capacity.saturating_sub(1)).ok()?;
        Some((next, highest.min(next.saturating_add(width))))
    }
    /// Begin one fenced attempt. Count is lifetime-of-tracker, survives reset,
    /// and is consumed before external work; no background retry is started.
    ///
    /// # Errors
    /// Returns `Recovering` unless an incident exists, `RecoveryInFlight`,
    /// terminal state, epoch/clock failure, or `Exhausted` at the attempt limit.
    pub fn begin_recovery(
        &mut self,
        epoch: FeedEpoch,
        at: PreciseInstant,
    ) -> Result<RecoveryTicket, ContinuityError> {
        self.epoch(epoch)?;
        self.advance_clock(at)?;
        match self.state {
            ContinuityState::Halted => return Err(ContinuityError::Halted),
            ContinuityState::Exhausted => return Err(ContinuityError::Exhausted),
            ContinuityState::Tracking => return Err(ContinuityError::Recovering),
            ContinuityState::Recovering => {}
        }
        if self.active.is_some() {
            return Err(ContinuityError::RecoveryInFlight);
        }
        if self.attempts >= self.policy.attempts {
            self.state = ContinuityState::Exhausted;
            return Err(ContinuityError::Exhausted);
        }
        self.attempts += 1;
        let ticket = RecoveryTicket {
            feed: self.scope.id,
            epoch,
            attempt: self.attempts,
        };
        self.active = Some(ActiveRecovery {
            ticket,
            started: at,
            rows: 0,
        });
        Ok(ticket)
    }
    /// Complete an attempt only after known holes and heartbeat loss are repaired.
    /// An unsuccessful completion still consumes this attempt and its fence.
    ///
    /// # Errors
    /// Returns `StaleRecovery` for an expired/old ticket, or a typed remaining barrier.
    pub fn finish_recovery(
        &mut self,
        ticket: RecoveryTicket,
        at: PreciseInstant,
    ) -> Result<(), ContinuityError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.ticket != ticket)
        {
            return Err(ContinuityError::StaleRecovery);
        }
        self.advance_clock(at)?;
        self.take_ticket(ticket)?;
        if self.missing_range().is_some() {
            return Err(ContinuityError::Gap);
        }
        let heartbeat = self.heartbeat.ok_or(ContinuityError::HeartbeatMissing)?;
        if elapsed(at, heartbeat).is_none_or(|age| age > self.policy.heartbeat_age.as_nanos()) {
            return Err(ContinuityError::HeartbeatGap);
        }
        self.check_watermark()?;
        self.state = ContinuityState::Tracking;
        Ok(())
    }
    /// Accept recovery output only under its live feed/epoch/attempt fence.
    /// Old or expired results cannot consume a newer attempt's clock or row work.
    /// Live intake uses `ingest`; recovery adapters must use this fenced path.
    ///
    /// # Errors
    /// Returns `StaleRecovery` without applying old work, or the normal intake error.
    pub fn ingest_recovered(
        &mut self,
        ticket: RecoveryTicket,
        at: PreciseInstant,
        record: CanonicalObservation,
    ) -> Result<super::IngestOutcome, ContinuityError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.ticket != ticket)
        {
            return Err(ContinuityError::StaleRecovery);
        }
        self.advance_clock(at)?;
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.ticket != ticket)
        {
            return Err(ContinuityError::StaleRecovery);
        }
        self.ingest(ticket.epoch, at, record)
    }
    /// Explicitly abandon an uncertain/cancelled attempt without refund or recovery.
    /// Dropping a ticket alone performs no state change or external compensation.
    ///
    /// # Errors
    /// Returns `StaleRecovery` if the fence no longer owns the active attempt.
    pub fn abandon_recovery(&mut self, ticket: RecoveryTicket) -> Result<(), ContinuityError> {
        self.take_ticket(ticket)
    }
    fn take_ticket(&mut self, ticket: RecoveryTicket) -> Result<(), ContinuityError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.ticket != ticket)
        {
            return Err(ContinuityError::StaleRecovery);
        }
        self.active = None;
        Ok(())
    }
    /// Fence old work and clear only this bounded window. Budget is never refunded,
    /// source scope/policy cannot change, and a new explicit recovery is mandatory.
    ///
    /// # Errors
    /// Returns `Epoch` unless increasing, `Clock` on regression or `ContinuityUnknown`
    /// for an unsupported sequence interpretation. Exhausted budget stays exhausted.
    pub fn reset(
        &mut self,
        epoch: FeedEpoch,
        mode: ContinuityMode,
        at: PreciseInstant,
    ) -> Result<(), ContinuityError> {
        if epoch <= self.epoch {
            return Err(ContinuityError::Epoch);
        }
        Self::validate_mode(self.scope, mode)?;
        self.advance_clock(at)?;
        self.epoch = epoch;
        self.mode = mode;
        self.started = at;
        self.heartbeat = None;
        self.watermark = None;
        self.active = None;
        self.records.clear();
        self.versions.clear();
        self.current.clear();
        self.state = if self.attempts >= self.policy.attempts {
            ContinuityState::Exhausted
        } else {
            ContinuityState::Recovering
        };
        Ok(())
    }
}
