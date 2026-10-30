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
}
impl Reason {
    /// Stable diagnostic code, without market values or caller data.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InstrumentNotAllowed => "instrument_not_allowed",
            Self::DirectionNotAllowed => "direction_not_allowed",
            Self::HorizonExceeded => "horizon_exceeded",
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
}
/// Sealed deterministic decision, retaining all applicable reasons in stable order.
#[derive(Debug, Eq, PartialEq)]
pub struct Decision {
    revision: [u8; 32],
    status: Status,
    eligible: Vec<InstrumentId>,
    reasons: Vec<Reason>,
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
        })
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
        })
    }
}
