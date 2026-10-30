//! Pure independent eligibility controls; no strategy, account-risk or approval authority.
use edgeagent_market_data_domain::InstrumentId;
/// Invalid policy or intent shape, distinct from a valid policy denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidInput;
impl std::fmt::Display for InvalidInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research eligibility input")
    }
}
impl std::error::Error for InvalidInput {}
/// Explicit requested direction; unsupported valid directions remain representable for denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Long-only research.
    Long,
    /// Short research, denied by this revision.
    Short,
}
/// Explicit intent. Holding horizon is not lookback or artifact TTL.
#[derive(Clone, Copy)]
pub struct Intent {
    /// Requested research direction.
    pub direction: Direction,
    /// Positive holding duration in trading sessions.
    pub horizon_sessions: u16,
}
/// Stable reason precedence in declaration order, independent of input order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Reason {
    /// One or more selected instruments are outside the policy allowlist.
    InstrumentNotAllowed,
    /// This policy permits only long research.
    DirectionNotAllowed,
    /// Valid holding horizon exceeds the inclusive policy ceiling.
    HorizonExceeded,
    /// One or more candidates fall below the explicit inclusive liquidity minimum.
    LiquidityBelowMinimum,
    /// Selected population volatility exceeds the inclusive ceiling.
    VolatilityExceeded,
    /// Selected maximum opening gap exceeds the inclusive ceiling.
    GapExceeded,
    /// An event intersects the selected closed exclusion window.
    EventExcluded,
    /// Exact endpoint regime is outside the explicit allowed set.
    RegimeNotAllowed,
}
impl Reason {
    /// Stable diagnostic code, without market values or caller data.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InstrumentNotAllowed => "instrument_not_allowed",
            Self::DirectionNotAllowed => "direction_not_allowed",
            Self::HorizonExceeded => "horizon_exceeded",
            Self::LiquidityBelowMinimum => "liquidity_below_minimum",
            Self::VolatilityExceeded => "volatility_exceeded",
            Self::GapExceeded => "gap_exceeded",
            Self::EventExcluded => "event_excluded",
            Self::RegimeNotAllowed => "regime_not_allowed",
        }
    }
}
/// These eligibility controls only; never strategy approval or publication permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// All candidates pass these controls.
    Approved,
    /// A nonempty subset passes these controls.
    Narrowed,
    /// No candidate may proceed.
    Abstained,
}
/// Immutable policy with exact version and configuration bindings.
pub struct Policy {
    revision: [u8; 32],
    configuration: [u8; 32],
    instruments: Vec<InstrumentId>,
    max_horizon: u16,
    liquidity: Option<u64>,
    market: Option<(u32, u32)>,
    context: Option<crate::context_policy::Rules>,
}
/// Sealed deterministic decision, retaining all applicable reasons in stable order.
#[derive(Debug, Eq, PartialEq)]
pub struct Decision {
    revision: [u8; 32],
    status: Status,
    eligible: Vec<InstrumentId>,
    reasons: Vec<Reason>,
    candidates: Vec<InstrumentId>,
}
impl Decision {
    /// Exact immutable policy revision.
    #[must_use]
    pub const fn revision(&self) -> [u8; 32] {
        self.revision
    }
    /// Result of these eligibility controls only.
    #[must_use]
    pub const fn status(&self) -> Status {
        self.status
    }
    /// Eligible identities in ascending order; never scores or replacement observations.
    #[must_use]
    pub fn eligible(&self) -> &[InstrumentId] {
        &self.eligible
    }
    /// All applicable reasons, sorted by the declared precedence.
    #[must_use]
    pub fn reasons(&self) -> &[Reason] {
        &self.reasons
    }
}
impl Policy {
    /// Construct explicit bounded policy; selections must be sorted and unique.
    /// # Errors
    /// Rejects zero revision sentinels, empty/oversize/duplicate selections and invalid ceilings.
    pub fn new(
        revision: [u8; 32],
        configuration: [u8; 32],
        instruments: Vec<InstrumentId>,
        max_horizon: u16,
    ) -> Result<Self, InvalidInput> {
        if revision == [0; 32]
            || configuration == [0; 32]
            || instruments.is_empty()
            || instruments.len() > crate::MAX_INSTRUMENTS
            || !instruments.windows(2).all(|w| w[0] < w[1])
            || !(1..=252).contains(&max_horizon)
        {
            return Err(InvalidInput);
        }
        Ok(Self {
            revision,
            configuration,
            instruments,
            max_horizon,
            liquidity: None,
            market: None,
            context: None,
        })
    }
    /// Require the compiled liquidity revision and explicit minimum; zero is not a disable flag.
    pub fn with_liquidity(
        mut self,
        revision: &str,
        minimum_cents: u64,
    ) -> Result<Self, InvalidInput> {
        if revision != crate::liquidity::REVISION {
            return Err(InvalidInput);
        }
        self.liquidity = Some(minimum_cents);
        Ok(self)
    }
    /// Whether selected configuration requires liquidity evidence.
    #[must_use]
    pub const fn requires_liquidity(&self) -> bool {
        self.liquidity.is_some()
    }
    /// Apply sealed metrics for every original candidate, preserving previous denials.
    /// # Errors
    /// Rejects a foreign decision, missing/duplicate/foreign metrics or an unconfigured gate.
    pub fn apply_liquidity(
        &self,
        mut decision: Decision,
        values: &[crate::liquidity::Liquidity],
    ) -> Result<Decision, InvalidInput> {
        let minimum = self.liquidity.ok_or(InvalidInput)?;
        if values.len() != decision.candidates.len() {
            return Err(InvalidInput);
        }
        let mut ids = values.iter().map(|v| v.instrument()).collect::<Vec<_>>();
        ids.sort_unstable();
        if decision.revision != self.revision || ids != decision.candidates {
            return Err(InvalidInput);
        }
        if values.iter().any(|v| v.cents() < minimum) {
            if !decision.reasons.contains(&Reason::LiquidityBelowMinimum) {
                decision.reasons.push(Reason::LiquidityBelowMinimum);
            }
            decision.eligible.retain(|id| {
                values
                    .iter()
                    .any(|v| v.instrument() == *id && v.cents() >= minimum)
            });
        }
        decision.status = if decision.eligible.is_empty() {
            Status::Abstained
        } else if decision.eligible.len() < decision.candidates.len() {
            Status::Narrowed
        } else {
            Status::Approved
        };
        Ok(decision)
    }
    /// Select both exact market revisions and explicit inclusive ppm maxima.
    /// # Errors
    /// Unsupported formula revisions reject rather than defaulting to another calculation.
    pub fn with_market_constraints(
        mut self,
        volatility: &str,
        gap: &str,
        max_volatility: u32,
        max_gap: u32,
    ) -> Result<Self, InvalidInput> {
        if volatility != crate::market_constraints::VOLATILITY_REVISION
            || gap != crate::market_constraints::GAP_REVISION
        {
            return Err(InvalidInput);
        }
        self.market = Some((max_volatility, max_gap));
        Ok(self)
    }
    /// Whether the selected policy requires complete opening evidence.
    #[must_use]
    pub const fn requires_market_constraints(&self) -> bool {
        self.market.is_some()
    }
    /// Apply metrics for the complete original universe, preserving earlier denials.
    /// # Errors
    /// Rejects absent configuration, foreign decisions and missing/duplicate/foreign metrics.
    pub fn apply_market_constraints(
        &self,
        mut decision: Decision,
        values: &[crate::market_constraints::Metrics],
    ) -> Result<Decision, InvalidInput> {
        let (vol, gap) = self.market.ok_or(InvalidInput)?;
        if values.len() != decision.candidates.len() {
            return Err(InvalidInput);
        }
        let mut ids = values.iter().map(|v| v.instrument()).collect::<Vec<_>>();
        ids.sort_unstable();
        if decision.revision != self.revision || ids != decision.candidates {
            return Err(InvalidInput);
        }
        for (failed, reason) in [
            (
                values.iter().any(|v| v.volatility_ppm() > vol),
                Reason::VolatilityExceeded,
            ),
            (
                values.iter().any(|v| v.gap_ppm() > gap),
                Reason::GapExceeded,
            ),
        ] {
            if failed && !decision.reasons.contains(&reason) {
                decision.reasons.push(reason);
            }
        }
        decision.reasons.sort_unstable();
        decision.eligible.retain(|id| {
            values
                .iter()
                .any(|v| v.instrument() == *id && v.volatility_ppm() <= vol && v.gap_ppm() <= gap)
        });
        decision.status = if decision.eligible.is_empty() {
            Status::Abstained
        } else if decision.eligible.len() < decision.candidates.len() {
            Status::Narrowed
        } else {
            Status::Approved
        };
        Ok(decision)
    }
    /// Select exact context revisions and explicit immutable rule parameters.
    /// # Errors
    /// Unsupported event, regime or quality revisions reject without fallback.
    pub fn with_context(
        mut self,
        event: &str,
        regime: &str,
        quality: &str,
        rules: crate::context_policy::Rules,
    ) -> Result<Self, InvalidInput> {
        use crate::context_policy as c;
        if event != c::EVENT_REVISION
            || regime != c::REGIME_REVISION
            || quality != c::QUALITY_REVISION
        {
            return Err(InvalidInput);
        }
        self.context = Some(rules);
        Ok(self)
    }
    /// Selected context rules, if this staged policy requires them.
    #[must_use]
    pub fn context(&self) -> Option<&crate::context_policy::Rules> {
        self.context.as_ref()
    }
    /// Apply sealed facts for every original candidate, retaining prior denials.
    /// # Errors
    /// Rejects absent configuration, foreign decisions or incomplete/duplicate fact census.
    pub fn apply_context(
        &self,
        mut decision: Decision,
        values: &[crate::context_policy::Facts],
    ) -> Result<Decision, InvalidInput> {
        let rules = self.context.as_ref().ok_or(InvalidInput)?;
        if values.len() != decision.candidates.len() {
            return Err(InvalidInput);
        }
        let mut ids = values.iter().map(|v| v.instrument()).collect::<Vec<_>>();
        ids.sort_unstable();
        if decision.revision != self.revision || ids != decision.candidates {
            return Err(InvalidInput);
        }
        for (failed, reason) in [
            (
                values.iter().any(|v| v.event_conflict()),
                Reason::EventExcluded,
            ),
            (
                values.iter().any(|v| !rules.allows(v.regime())),
                Reason::RegimeNotAllowed,
            ),
        ] {
            if failed && !decision.reasons.contains(&reason) {
                decision.reasons.push(reason);
            }
        }
        decision.reasons.sort_unstable();
        decision.eligible.retain(|id| {
            values
                .iter()
                .any(|v| v.instrument() == *id && !v.event_conflict() && rules.allows(v.regime()))
        });
        decision.status = if decision.eligible.is_empty() {
            Status::Abstained
        } else if decision.eligible.len() < decision.candidates.len() {
            Status::Narrowed
        } else {
            Status::Approved
        };
        Ok(decision)
    }
    /// Exact selected configuration identity; callers must authenticate its resolution.
    #[must_use]
    pub const fn configuration(&self) -> [u8; 32] {
        self.configuration
    }
    /// Assess a complete selected universe without silently dropping malformed input.
    /// # Errors
    /// Rejects empty/oversize/duplicate candidates or a horizon outside 1..=252 sessions.
    pub fn assess(
        &self,
        candidates: &[InstrumentId],
        intent: Intent,
    ) -> Result<Decision, InvalidInput> {
        if candidates.is_empty()
            || candidates.len() > crate::MAX_INSTRUMENTS
            || !(1..=252).contains(&intent.horizon_sessions)
        {
            return Err(InvalidInput);
        }
        let mut ordered = candidates.to_vec();
        ordered.sort_unstable();
        if ordered.windows(2).any(|w| w[0] == w[1]) {
            return Err(InvalidInput);
        }
        let mut eligible: Vec<_> = ordered
            .iter()
            .copied()
            .filter(|id| self.instruments.binary_search(id).is_ok())
            .collect();
        let mut reasons = Vec::new();
        if eligible.len() != ordered.len() {
            reasons.push(Reason::InstrumentNotAllowed);
        }
        if intent.direction != Direction::Long {
            reasons.push(Reason::DirectionNotAllowed);
        }
        if intent.horizon_sessions > self.max_horizon {
            reasons.push(Reason::HorizonExceeded);
        }
        if intent.direction != Direction::Long || intent.horizon_sessions > self.max_horizon {
            eligible.clear();
        }
        let status = if eligible.is_empty() {
            Status::Abstained
        } else if eligible.len() < ordered.len() {
            Status::Narrowed
        } else {
            Status::Approved
        };
        Ok(Decision {
            revision: self.revision,
            status,
            eligible,
            reasons,
            candidates: ordered,
        })
    }
}
