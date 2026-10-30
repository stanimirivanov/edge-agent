//! Independent exact-price regimes, closed event windows and critical evidence freshness.
use crate::EligibleSeries;
use edgeagent_market_data_domain::InstrumentId;
/// Complete coverage of a closed millisecond event-exclusion window.
pub const EVENT_REVISION: &str = "complete-closed-event-window-v1";
/// Exact raw endpoint comparison; not a market-wide or learned regime.
pub const REGIME_REVISION: &str = "raw-endpoint-direction-v1";
/// Existing sealed owner checks plus independent market/context age ceiling.
pub const QUALITY_REVISION: &str = "owner-qualified-market-context-age-v1";
/// Stable exact-price classification order for allowed-regime declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Regime {
    /// Final close is below initial close.
    Falling,
    /// Endpoint prices are exactly equal.
    Flat,
    /// Final close is above initial close.
    Rising,
}
/// Critical context failure priority, independent of input order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Error {
    /// Coverage claims exceed retained use authority.
    MissingEntitlement,
    /// Binding, interval or event census is malformed.
    InvalidEvidence,
    /// Required context was known only after the decision.
    FutureEvidence,
    /// A critical market/context timestamp exceeds the age budget.
    StaleEvidence,
    /// Coverage is absent or does not cover the complete window.
    CoverageUnavailable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("research context refused")
    }
}
impl std::error::Error for Error {}
/// Exact owner coverage assertion; absence remains explicit.
pub enum Coverage {
    /// No completeness assertion exists.
    Unknown,
    /// Bounded sorted owner event census over a half-open interval.
    Complete {
        /// Immutable source specification digest.
        source: [u8; 32],
        /// Actual knowledge time, Unix milliseconds.
        known_at_ms: i64,
        /// Half-open coverage, Unix milliseconds.
        interval: [i64; 2],
        /// Sorted unique event instants, Unix milliseconds.
        events: Vec<i64>,
    },
}
/// Context asserted by the authorized reader of the same daily artifact.
pub struct Input {
    /// Stable instrument.
    pub instrument: InstrumentId,
    /// Exact retained daily digest.
    pub evidence: [u8; 32],
    /// Explicit event-calendar assertion.
    pub coverage: Coverage,
}
/// Explicit immutable rule parameters; no production defaults.
pub struct Rules {
    before: u32,
    after: u32,
    max_age: i64,
    allowed: Vec<Regime>,
}
/// Sealed facts, independently computed from qualified evidence.
pub struct Facts {
    instrument: InstrumentId,
    regime: Regime,
    event_conflict: bool,
}
impl Facts {
    /// Bound instrument.
    #[must_use]
    pub const fn instrument(&self) -> InstrumentId {
        self.instrument
    }
    /// Exact endpoint-price regime.
    #[must_use]
    pub const fn regime(&self) -> Regime {
        self.regime
    }
    /// Whether any event intersects the closed exclusion window.
    #[must_use]
    pub const fn event_conflict(&self) -> bool {
        self.event_conflict
    }
}
impl Rules {
    /// Validate an explicit window, age budget and sorted unique nonempty regime set.
    /// # Errors
    /// Rejects invalid age bounds and malformed allowed-regime declarations.
    pub fn new(
        before: u32,
        after: u32,
        max_age: i64,
        allowed: Vec<Regime>,
    ) -> Result<Self, crate::policy::InvalidInput> {
        if !(1..=2_678_400_000).contains(&max_age)
            || allowed.is_empty()
            || allowed.len() > 3
            || !allowed.windows(2).all(|w| w[0] < w[1])
        {
            return Err(crate::policy::InvalidInput);
        }
        Ok(Self {
            before,
            after,
            max_age,
            allowed,
        })
    }
    pub(crate) fn allows(&self, regime: Regime) -> bool {
        self.allowed.contains(&regime)
    }
    /// Assess exact binding, knowledge, freshness and complete coverage before returning facts.
    /// # Errors
    /// Every missing/invalid critical assertion refuses the whole request in the application.
    pub fn assess(
        &self,
        series: &EligibleSeries<'_>,
        input: &Input,
        decision: i64,
    ) -> Result<Facts, Error> {
        if decision < 0 {
            return Err(Error::InvalidEvidence);
        }
        if input.instrument != series.frozen().selected.instrument
            || input.evidence != series.frozen().evidence
        {
            return Err(Error::InvalidEvidence);
        }
        let first = series.closes().first().ok_or(Error::CoverageUnavailable)?;
        let last = series.closes().last().ok_or(Error::CoverageUnavailable)?;
        if series.frozen().rights_until_ms <= decision {
            return Err(Error::MissingEntitlement);
        }
        if series.frozen().known_at_ms > decision
            || series.closes().iter().any(|c| c.available_at_ms > decision)
        {
            return Err(Error::FutureEvidence);
        }
        let (source, known, interval, events) = match &input.coverage {
            Coverage::Unknown => {
                if decision - last.event_at_ms > self.max_age {
                    return Err(Error::StaleEvidence);
                }
                return Err(Error::CoverageUnavailable);
            }
            Coverage::Complete {
                source,
                known_at_ms,
                interval,
                events,
            } => (source, *known_at_ms, interval, events),
        };
        if *source == [0; 32]
            || known < 0
            || interval[0] < 0
            || interval[1] <= interval[0]
            || events.len() > 256
            || !events.windows(2).all(|w| w[0] < w[1])
            || events.iter().any(|t| *t < interval[0] || *t >= interval[1])
        {
            return Err(Error::InvalidEvidence);
        }
        if interval[1] > series.frozen().rights_until_ms {
            return Err(Error::MissingEntitlement);
        }
        if known > decision || last.event_at_ms > decision {
            return Err(Error::FutureEvidence);
        }
        if decision - known > self.max_age || decision - last.event_at_ms > self.max_age {
            return Err(Error::StaleEvidence);
        }
        let start = decision
            .checked_sub(i64::from(self.before))
            .filter(|v| *v >= 0)
            .ok_or(Error::CoverageUnavailable)?;
        let end = decision
            .checked_add(i64::from(self.after))
            .ok_or(Error::InvalidEvidence)?;
        if interval[0] > start || interval[1] <= end {
            return Err(Error::CoverageUnavailable);
        }
        let regime = match last.close.cmp(&first.close) {
            std::cmp::Ordering::Less => Regime::Falling,
            std::cmp::Ordering::Equal => Regime::Flat,
            std::cmp::Ordering::Greater => Regime::Rising,
        };
        Ok(Facts {
            instrument: input.instrument,
            regime,
            event_conflict: events.iter().any(|t| *t >= start && *t <= end),
        })
    }
}
