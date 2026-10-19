use crate::{
    CanonicalObservation, ContinuityError, ContinuityTracker, EvidenceContract, FeedEpoch, FeedId,
    InstrumentId, ObservationId, ObservationRevision, ObservationValue, PreciseInstant,
    SemanticReference, continuity::elapsed,
};
use std::time::Duration;

/// Continuity demanded by a consumer, independently of a provider declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContinuityRequirement {
    /// Snapshots are sufficient; no queue/depth or continuity claim follows.
    Snapshots,
    /// Require a qualified contiguous prefix and completed recovery barriers.
    Contiguous,
}
/// One independently declared required feed/instrument/semantic contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceRequirement {
    feed: FeedId,
    instrument: InstrumentId,
    contract: EvidenceContract,
    continuity: ContinuityRequirement,
}
impl EvidenceRequirement {
    /// Pin one required input; this is not a source/rights qualification grant.
    #[must_use]
    pub const fn new(
        feed: FeedId,
        instrument: InstrumentId,
        contract: EvidenceContract,
        continuity: ContinuityRequirement,
    ) -> Self {
        Self {
            feed,
            instrument,
            contract,
            continuity,
        }
    }
}
/// Candidate identity with expected epoch. The borrow prevents mutation of its
/// tracker while a successful readiness result still uses retained evidence.
#[derive(Clone, Copy, Debug)]
pub struct EvidenceInput<'a> {
    tracker: &'a ContinuityTracker,
    epoch: FeedEpoch,
    id: ObservationId,
    revision: ObservationRevision,
}
impl<'a> EvidenceInput<'a> {
    /// Describe a retained candidate; policy evaluation performs all checks.
    #[must_use]
    pub const fn new(
        tracker: &'a ContinuityTracker,
        epoch: FeedEpoch,
        id: ObservationId,
        revision: ObservationRevision,
    ) -> Self {
        Self {
            tracker,
            epoch,
            id,
            revision,
        }
    }
}
/// Explicit bounded point-in-time readiness plan, not an execution permission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessPolicy {
    revision: SemanticReference,
    max_age: Duration,
    max_skew: Duration,
    required: Vec<EvidenceRequirement>,
}
impl ReadinessPolicy {
    /// Require 1..=32 distinct feed/instrument requirements and a positive source
    /// age bound. Zero skew means exact alignment; there are no timing defaults.
    ///
    /// # Errors
    /// Returns `Policy` before allocating for invalid cardinality, age or duplicates.
    pub fn new(
        revision: SemanticReference,
        max_age: Duration,
        max_skew: Duration,
        required: &[EvidenceRequirement],
    ) -> Result<Self, ContinuityError> {
        if required.is_empty()
            || required.len() > 32
            || max_age.is_zero()
            || required.iter().enumerate().any(|(i, item)| {
                required
                    .iter()
                    .take(i)
                    .any(|old| (old.feed, old.instrument) == (item.feed, item.instrument))
            })
        {
            return Err(ContinuityError::Policy);
        }
        Ok(Self {
            revision,
            max_age,
            max_skew,
            required: required.to_vec(),
        })
    }
    /// Check every required input, returning observations in declared plan order.
    /// Bars use bucket end, not a start label, for age/skew. Quotes check the
    /// snapshot and both side times. Local receipt/heartbeat never freshens prices.
    ///
    /// # Errors
    /// Returns typed missing/duplicate, epoch, scope, continuity, supersession,
    /// availability, freshness, skew or health failures. This proves neither
    /// rights, full calendar/reference quality, liquidity nor profile eligibility.
    pub fn check<'a>(
        &self,
        at: PreciseInstant,
        inputs: &[EvidenceInput<'a>],
    ) -> Result<ReadyEvidence<'a>, ContinuityError> {
        if inputs.len() != self.required.len() {
            return Err(ContinuityError::Input);
        }
        for (i, input) in inputs.iter().enumerate() {
            if input.tracker.identity().1 != input.epoch {
                return Err(ContinuityError::Epoch);
            }
            for other in inputs.iter().take(i) {
                if input.tracker.identity().0.binding().0 == other.tracker.identity().0.binding().0
                    && !std::ptr::eq(input.tracker, other.tracker)
                {
                    return Err(ContinuityError::Input);
                }
            }
        }
        let mut observations = Vec::with_capacity(self.required.len());
        let mut feeds = Vec::with_capacity(self.required.len());
        let mut extremes: Option<(PreciseInstant, PreciseInstant)> = None;
        for required in &self.required {
            let mut selected = None;
            for input in inputs {
                if input.tracker.identity().0.binding().0 != required.feed {
                    continue;
                }
                let record = input.tracker.selected(input.id, input.revision)?;
                if ContinuityTracker::instrument(record) != required.instrument {
                    continue;
                }
                if selected.is_some() {
                    return Err(ContinuityError::Input);
                }
                input
                    .tracker
                    .check_readiness(input.epoch, at, required.continuity)?;
                if record.context().origin().1.declaration().0 != required.contract {
                    return Err(ContinuityError::Scope);
                }
                selected = Some((record, input.tracker.identity()));
            }
            let (record, (scope, epoch, continuity_policy)) =
                selected.ok_or(ContinuityError::Input)?;
            feeds.push((scope.binding().0, epoch, continuity_policy));
            let source_times = match record.value() {
                Some(ObservationValue::Bar(bar)) => [Some(bar.bucket().0.bounds().1), None, None],
                Some(ObservationValue::OptionQuote(quote)) => [
                    Some(record.times().event_at()),
                    Some(quote.sides().0.at()),
                    Some(quote.sides().1.at()),
                ],
                _ => [Some(record.times().event_at()), None, None],
            };
            for time in source_times.into_iter().flatten() {
                if elapsed(at, time).ok_or(ContinuityError::Clock)? > self.max_age.as_nanos() {
                    return Err(ContinuityError::Stale);
                }
                extremes = Some(match extremes {
                    None => (time, time),
                    Some((low, high)) => (low.min(time), high.max(time)),
                });
            }
            observations.push(record);
        }
        if extremes.is_some_and(|(low, high)| {
            elapsed(high, low).is_none_or(|skew| skew > self.max_skew.as_nanos())
        }) {
            return Err(ContinuityError::Skew);
        }
        Ok(ReadyEvidence {
            at,
            revision: self.revision,
            observations,
            feeds,
        })
    }
}
impl ContinuityTracker {
    pub(crate) fn check_readiness(
        &self,
        epoch: FeedEpoch,
        at: PreciseInstant,
        requirement: ContinuityRequirement,
    ) -> Result<(), ContinuityError> {
        if self.identity().1 != epoch {
            return Err(ContinuityError::Epoch);
        }
        self.health(at)?;
        if requirement == ContinuityRequirement::Contiguous && self.mode_is_snapshot() {
            return Err(ContinuityError::ContinuityUnknown);
        }
        Ok(())
    }
}

/// Borrowed, point-in-time evidence result, not a serializable authorization.
/// It cannot outlive the retained values or be used across tracker mutation.
///
/// ```compile_fail,E0502
/// use edgeagent_market_data_domain::*;
/// fn invalid(tracker: &mut ContinuityTracker, policy: &ReadinessPolicy,
///     at: PreciseInstant, epoch: FeedEpoch, id: ObservationId,
///     revision: ObservationRevision) -> Result<(), ContinuityError> {
///     let inputs = [EvidenceInput::new(tracker, epoch, id, revision)];
///     let ready = policy.check(at, &inputs)?;
///     tracker.reset(epoch, ContinuityMode::Snapshots, at)?;
///     let _ = ready.observations();
///     Ok(())
/// }
/// ```
pub struct ReadyEvidence<'a> {
    at: PreciseInstant,
    revision: SemanticReference,
    observations: Vec<&'a CanonicalObservation>,
    feeds: Vec<(FeedId, FeedEpoch, SemanticReference)>,
}
impl<'a> ReadyEvidence<'a> {
    /// Checked evaluation instant and readiness-plan revision.
    #[must_use]
    pub const fn assessment(&self) -> (PreciseInstant, SemanticReference) {
        (self.at, self.revision)
    }
    /// Accepted current observations in required-plan order, never buffered/cancelled.
    #[must_use]
    pub fn observations(&self) -> &[&'a CanonicalObservation] {
        &self.observations
    }
    /// Feed identity, epoch and continuity-policy revision for each observation,
    /// in the same declared order; retain these alongside future run evidence.
    #[must_use]
    pub fn feeds(&self) -> &[(FeedId, FeedEpoch, SemanticReference)] {
        &self.feeds
    }
}
impl std::fmt::Debug for ReadyEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadyEvidence")
            .field("inputs", &self.observations.len())
            .finish_non_exhaustive()
    }
}
