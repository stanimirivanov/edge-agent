//! Pure minimum-evidence policy. No clock, I/O, transport DTO or provider authority.
//! Applications must authenticate owner assertions and verify retained bytes before use.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};

/// Structural upper bound matching the research request universe ceiling.
pub const MAX_INSTRUMENTS: usize = 64;
/// Stable abstention priority, highest priority first. Never depends on input order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Abstention {
    /// Authenticated owner says use is denied or entitlement expired.
    MissingEntitlement,
    /// Request contains an instrument outside the explicitly supported universe.
    UnsupportedUniverse,
    /// Conflicting, malformed or incompatible frozen evidence.
    InvalidEvidence,
    /// Some observation or required owner knowledge is after the decision cutoff.
    FutureEvidence,
    /// Latest event close is older than the selected policy permits.
    StaleEvidence,
    /// Required instrument or historical session is absent.
    InsufficientHistory,
}
impl Abstention {
    /// Exact M04 v1 wire-reason spelling; serialization belongs to an outer adapter.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingEntitlement => "missing_entitlement",
            Self::UnsupportedUniverse => "unsupported_universe",
            Self::InvalidEvidence => "invalid_evidence",
            Self::FutureEvidence => "future_evidence",
            Self::StaleEvidence => "stale_evidence",
            Self::InsufficientHistory => "insufficient_history",
        }
    }
}
/// Invalid policy or request shape; not an accepted-request abstention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSelection;
impl std::fmt::Display for InvalidSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research evidence selection")
    }
}
impl std::error::Error for InvalidSelection {}
/// Explicit basis: this first policy qualifies raw USD closes only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentBasis {
    /// Raw USD prices; corporate actions must already have passed owner quality policy.
    Raw,
    /// Representable as input, but deliberately unsupported by this policy.
    SplitAdjusted,
}
/// Historical instrument/listing mapping selected at the decision cutoff.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SelectedInstrument {
    /// Stable instrument identity.
    pub instrument: InstrumentId,
    /// Historical listing identity, not current universe membership.
    pub listing: ListingId,
    /// Exact owner-reviewed mapping artifact revision digest.
    pub mapping: [u8; 32],
}
/// Validated USD price with explicit chronology. Raw input until assessed.
#[derive(Clone)]
pub struct FrozenClose {
    /// Explicit exchange session; a date alone is not calendar qualification.
    pub session: SessionDate,
    /// Unadjusted USD close; decimal units are provided by the existing value type.
    pub close: UsdPrice,
    /// End of this observation's market bucket in Unix milliseconds.
    pub event_at_ms: i64,
    /// Source observation receipt in Unix milliseconds.
    pub observed_at_ms: i64,
    /// Availability including processing/readiness delay in Unix milliseconds.
    pub available_at_ms: i64,
}
/// Owner-exported frozen series. These fields assert facts; they grant no authority.
#[derive(Clone)]
pub struct FrozenSeries {
    /// Historical instrument and mapping binding.
    pub selected: SelectedInstrument,
    /// Exact retained evidence artifact digest; adapter must verify the actual bytes.
    pub evidence: [u8; 32],
    /// Implementation revision that interpreted the frozen evidence.
    pub implementation: [u8; 32],
    /// Selected immutable research/evidence/numeric configuration.
    pub configuration: [u8; 32],
    /// Calendar artifact revision matching the selected session census.
    pub calendar: [u8; 32],
    /// Receipt time of the mapping/calendar/quality/rights assertions.
    pub known_at_ms: i64,
    /// Exclusive expiry of the owner-asserted right to use this evidence.
    pub rights_until_ms: i64,
    /// Owner-asserted audience/scope entitlement after adapter authentication.
    pub entitled: bool,
    /// Owner quality result, including action coverage; not recomputed here.
    pub quality_accepted: bool,
    /// Declared price adjustment basis.
    pub basis: AdjustmentBasis,
    /// One close per required session. Input ordering does not select revisions.
    pub closes: Vec<FrozenClose>,
}
/// Owner-selected immutable revisions and explicit decision/freshness bounds.
#[derive(Clone, Copy)]
pub struct PolicyBinding {
    /// Unix millisecond decision cutoff.
    pub decision_at_ms: i64,
    /// Inclusive maximum age of the latest market close, in milliseconds.
    pub max_age_ms: i64,
    /// Expected interpreting implementation digest.
    pub implementation: [u8; 32],
    /// Expected policy configuration digest.
    pub configuration: [u8; 32],
    /// Expected historical calendar artifact digest.
    pub calendar: [u8; 32],
}
/// Immutable selected policy and historical session/universe census.
pub struct EvidencePolicy {
    decision_at_ms: i64,
    max_age_ms: i64,
    implementation: [u8; 32],
    configuration: [u8; 32],
    calendar: [u8; 32],
    universe: Vec<SelectedInstrument>,
    sessions: Vec<SessionDate>,
}
fn digest_valid(value: &[u8; 32]) -> bool {
    value.iter().any(|v| *v != 0)
}
impl EvidencePolicy {
    /// Construct one bounded, explicitly selected historical research policy.
    /// `sessions` must contain exactly lookback+1 ascending sessions. No calendar is inferred.
    /// # Errors
    /// Rejects malformed/duplicate selection, zero revision sentinels and invalid time bounds.
    pub fn new(
        binding: PolicyBinding,
        universe: Vec<SelectedInstrument>,
        sessions: Vec<SessionDate>,
        lookback_sessions: u16,
    ) -> Result<Self, InvalidSelection> {
        let PolicyBinding {
            decision_at_ms,
            max_age_ms,
            implementation,
            configuration,
            calendar,
        } = binding;
        if decision_at_ms < 0
            || max_age_ms <= 0
            || !(1..=252).contains(&lookback_sessions)
            || !digest_valid(&implementation)
            || !digest_valid(&configuration)
            || !digest_valid(&calendar)
            || universe.is_empty()
            || universe.len() > MAX_INSTRUMENTS
            || universe.iter().any(|v| !digest_valid(&v.mapping))
            || !universe
                .windows(2)
                .all(|v| v[0].instrument < v[1].instrument)
            || sessions.len() != usize::from(lookback_sessions) + 1
            || !sessions.windows(2).all(|v| v[0] < v[1])
        {
            return Err(InvalidSelection);
        }
        Ok(Self {
            decision_at_ms,
            max_age_ms,
            implementation,
            configuration,
            calendar,
            universe,
            sessions,
        })
    }
    /// Assess one frozen series per selected instrument, returning a sealed ordered view.
    /// This does not authenticate claims, establish licenses or calculate momentum.
    /// Every failure is accumulated with fixed priority; no partial success is returned.
    pub fn assess<'a>(
        &self,
        inputs: &'a [FrozenSeries],
    ) -> Result<EligibleEvidence<'a>, Abstention> {
        let mut reason = None;
        let mut fail = |r| {
            reason = Some(reason.map_or(r, |old: Abstention| old.min(r)));
        };
        if inputs.len() > MAX_INSTRUMENTS {
            return Err(Abstention::InvalidEvidence);
        }
        for series in inputs {
            if !series.entitled || series.rights_until_ms <= self.decision_at_ms {
                fail(Abstention::MissingEntitlement);
            }
            if !self
                .universe
                .iter()
                .any(|v| v.instrument == series.selected.instrument)
            {
                fail(Abstention::UnsupportedUniverse);
            }
        }
        let mut ordered = Vec::with_capacity(self.universe.len());
        for selected in &self.universe {
            let candidates: Vec<_> = inputs
                .iter()
                .filter(|v| v.selected.instrument == selected.instrument)
                .collect();
            if candidates.is_empty() {
                fail(Abstention::InsufficientHistory);
                continue;
            }
            if candidates.len() != 1 {
                fail(Abstention::InvalidEvidence);
                continue;
            }
            let series = candidates[0];
            if series.selected != *selected
                || !digest_valid(&series.evidence)
                || series.implementation != self.implementation
                || series.configuration != self.configuration
                || series.calendar != self.calendar
                || !series.quality_accepted
                || series.basis != AdjustmentBasis::Raw
                || series.known_at_ms < 0
            {
                fail(Abstention::InvalidEvidence);
            }
            if series.known_at_ms > self.decision_at_ms {
                fail(Abstention::FutureEvidence);
            }
            if series.closes.len() > self.sessions.len() {
                fail(Abstention::InvalidEvidence);
                continue;
            }
            let mut closes: Vec<_> = series.closes.iter().collect();
            closes.sort_by_key(|v| v.session);
            if closes.windows(2).any(|v| v[0].session == v[1].session) {
                fail(Abstention::InvalidEvidence);
            }
            if closes.iter().any(|v| !self.sessions.contains(&v.session)) {
                fail(Abstention::InvalidEvidence);
            }
            if closes.len() != self.sessions.len() {
                fail(Abstention::InsufficientHistory);
            }
            for close in &closes {
                if close.close.decimal_parts().0 <= 0
                    || close.event_at_ms < 0
                    || close.observed_at_ms < close.event_at_ms
                    || close.available_at_ms < close.observed_at_ms
                    || close.available_at_ms < series.known_at_ms
                {
                    fail(Abstention::InvalidEvidence);
                }
                if close.available_at_ms > self.decision_at_ms {
                    fail(Abstention::FutureEvidence);
                }
            }
            if closes
                .windows(2)
                .any(|v| v[0].event_at_ms >= v[1].event_at_ms)
            {
                fail(Abstention::InvalidEvidence);
            }
            if closes.last().is_some_and(|v| {
                v.event_at_ms <= self.decision_at_ms
                    && self.decision_at_ms - v.event_at_ms > self.max_age_ms
            }) {
                fail(Abstention::StaleEvidence);
            }
            ordered.push(EligibleSeries { series, closes });
        }
        match reason {
            Some(reason) => Err(reason),
            None => Ok(EligibleEvidence { series: ordered }),
        }
    }
}
/// Sealed complete view; cannot be constructed by callers or mutate retained inputs.
pub struct EligibleEvidence<'a> {
    series: Vec<EligibleSeries<'a>>,
}
impl<'a> EligibleEvidence<'a> {
    /// Candidates in the explicitly selected historical universe order.
    #[must_use]
    pub fn series(&self) -> &[EligibleSeries<'a>] {
        &self.series
    }
}
/// Validated series retaining its exact evidence/revision metadata and ordered closes.
pub struct EligibleSeries<'a> {
    series: &'a FrozenSeries,
    closes: Vec<&'a FrozenClose>,
}
impl<'a> EligibleSeries<'a> {
    /// Original immutable owner assertions, for lineage retention.
    #[must_use]
    pub const fn frozen(&self) -> &'a FrozenSeries {
        self.series
    }
    /// Complete ascending-session close census for later deterministic calculation.
    #[must_use]
    pub fn closes(&self) -> &[&'a FrozenClose] {
        &self.closes
    }
}
