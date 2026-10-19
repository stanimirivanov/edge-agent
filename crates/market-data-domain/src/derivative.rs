use std::fmt;

use crate::{
    Currency, EffectiveInterval, InstrumentId, OptionPoints, OptionSchedule, ReferenceError,
    ReferenceHistory, ReferenceId, ReferenceRevision, ReferenceVersion, UsdPerPoint, UtcInstant,
    reference::exactly_one,
};

/// Payload-free specification failures; no provider error text or licensed values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DerivativeError {
    /// Strike or premium tick must be positive.
    InvalidTerms,
    /// Option and underlying identities cannot be equal.
    SelfReference,
    /// Contract deadlines contradict their supported ordering.
    Schedule,
    /// Source timezone revision differs from the pinned conversion rules.
    TimezoneVersion,
    /// Converted local date is outside the supported year range.
    LocalDate,
    /// The modeled product is outside the initial cash/European/index shape.
    UnsupportedProfile,
}
impl fmt::Display for DerivativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTerms => "invalid option terms",
            Self::SelfReference => "option cannot reference itself",
            Self::Schedule => "inconsistent option deadlines",
            Self::TimezoneVersion => "unsupported timezone-data revision",
            Self::LocalDate => "unsupported local date",
            Self::UnsupportedProfile => "unsupported initial option simulation shape",
        })
    }
}
impl std::error::Error for DerivativeError {}

/// Stable underlying identity; an index level cannot silently become an ETF trade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Underlying {
    /// Common-equity underlying.
    Equity(InstrumentId),
    /// Exchange-traded fund underlying.
    Etf(InstrumentId),
    /// Non-tradable index requiring its own level/settlement observations.
    Index(InstrumentId),
}
impl Underlying {
    /// Identity shared with the underlying's independently qualified reference data.
    #[must_use]
    pub const fn id(self) -> InstrumentId {
        match self {
            Self::Equity(id) | Self::Etf(id) | Self::Index(id) => id,
        }
    }
}

/// Option payoff direction; no position or short-selling authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OptionKind {
    /// Call contract.
    Call,
    /// Put contract.
    Put,
}
/// Contract exercise style, not implemented exercise capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExerciseStyle {
    /// Exercise at the specified European deadline.
    European,
    /// American exercise requires separate lifecycle implementation.
    American,
}

/// Exact immutable deliverable-specification reference, not an assumed lot size
/// or evidence that the referenced deliverables are known/available. Physical
/// delivery remains unsupported; future consumers must resolve this exact version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliverableReference {
    id: ReferenceId,
    revision: ReferenceRevision,
}
impl DeliverableReference {
    /// Bind checked nonzero identity and revision without performing resolution.
    #[must_use]
    pub const fn new(id: ReferenceId, revision: ReferenceRevision) -> Self {
        Self { id, revision }
    }
    /// Identity and exact revision; never resolve implicitly to latest.
    #[must_use]
    pub const fn version(self) -> (ReferenceId, ReferenceRevision) {
        (self.id, self.revision)
    }
}
/// Settlement metadata; both variants are modeled but only cash is initially targeted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SettlementStyle {
    /// USD cash settlement under an explicit product rule.
    Cash,
    /// Delivery needs a known, versioned deliverable specification.
    Physical(DeliverableReference),
}

/// Constant zero-origin premium tick and exact contractual terms. Variable tick
/// schemes need a future qualified representation, not a guessed fixed increment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionTerms {
    kind: OptionKind,
    strike: OptionPoints,
    premium_tick: OptionPoints,
    multiplier: UsdPerPoint,
    exercise: ExerciseStyle,
    settlement: SettlementStyle,
}
impl OptionTerms {
    /// Build checked terms; strike and premium tick are distinct grids. In
    /// particular a strike need not be divisible by a premium-price tick.
    ///
    /// # Errors
    /// Returns `InvalidTerms` for zero strike or premium tick.
    pub fn new(
        kind: OptionKind,
        strike: OptionPoints,
        premium_tick: OptionPoints,
        multiplier: UsdPerPoint,
        exercise: ExerciseStyle,
        settlement: SettlementStyle,
    ) -> Result<Self, DerivativeError> {
        if strike.decimal_parts().0 == 0 || premium_tick.decimal_parts().0 == 0 {
            return Err(DerivativeError::InvalidTerms);
        }
        Ok(Self {
            kind,
            strike,
            premium_tick,
            multiplier,
            exercise,
            settlement,
        })
    }
    /// Call/put and positive strike in quotation points.
    #[must_use]
    pub const fn payoff(self) -> (OptionKind, OptionPoints) {
        (self.kind, self.strike)
    }
    /// Premium tick and USD per point per contract; neither is a share quantity.
    #[must_use]
    pub const fn quotation(self) -> (OptionPoints, UsdPerPoint, Currency) {
        (self.premium_tick, self.multiplier, Currency::Usd)
    }
    /// Exercise and settlement metadata, independently represented.
    #[must_use]
    pub const fn lifecycle(self) -> (ExerciseStyle, SettlementStyle) {
        (self.exercise, self.settlement)
    }
}

/// Dated option specification. Wrap in `ReferenceVersion` to retain source,
/// checksum, service revision and actual receipt/knowledge time across corrections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionSpecification {
    id: InstrumentId,
    underlying: Underlying,
    terms: OptionTerms,
    schedule: OptionSchedule,
    effective: EffectiveInterval,
}
impl OptionSpecification {
    /// Construct a reference value, not an admission or qualification decision.
    ///
    /// # Errors
    /// Returns `SelfReference` if the option names itself as underlying.
    pub fn new(
        id: InstrumentId,
        underlying: Underlying,
        terms: OptionTerms,
        schedule: OptionSchedule,
        effective: EffectiveInterval,
    ) -> Result<Self, DerivativeError> {
        if id == underlying.id() {
            return Err(DerivativeError::SelfReference);
        }
        Ok(Self {
            id,
            underlying,
            terms,
            schedule,
            effective,
        })
    }
    /// Stable option instrument identity.
    #[must_use]
    pub const fn id(self) -> InstrumentId {
        self.id
    }
    /// Explicit underlying identity and class, not a provider alias.
    #[must_use]
    pub const fn underlying(self) -> Underlying {
        self.underlying
    }
    /// Exact option terms.
    #[must_use]
    pub const fn terms(self) -> OptionTerms {
        self.terms
    }
    /// Explicit deadlines and pinned local-date semantics.
    #[must_use]
    pub const fn schedule(self) -> OptionSchedule {
        self.schedule
    }
    /// Half-open effective UTC interval, independent of reference knowledge time.
    #[must_use]
    pub const fn effective(self) -> EffectiveInterval {
        self.effective
    }
    /// Check only the initial cash/European/index product shape. Success grants
    /// no simulation eligibility: evidence, rights, calendar, risk, funding and
    /// actual runtime qualification remain mandatory later gates.
    ///
    /// # Errors
    /// Returns `UnsupportedProfile` for American, physical or non-index products.
    pub fn validate_initial_simulation_shape(self) -> Result<(), DerivativeError> {
        if matches!(self.underlying, Underlying::Index(_))
            && self.terms.exercise == ExerciseStyle::European
            && self.terms.settlement == SettlementStyle::Cash
        {
            Ok(())
        } else {
            Err(DerivativeError::UnsupportedProfile)
        }
    }
}
impl ReferenceHistory<OptionSpecification> {
    /// Resolve one specification by instrument/effective instant and inclusive
    /// knowledge cutoff. Later-known corrections cannot affect earlier reads.
    ///
    /// # Errors
    /// Returns `Missing` or `Ambiguous` reference coverage without guessing.
    pub fn resolve_option(
        &self,
        id: InstrumentId,
        effective_at: UtcInstant,
        cutoff: UtcInstant,
    ) -> Result<&ReferenceVersion<OptionSpecification>, ReferenceError> {
        exactly_one(
            self.at(cutoff)
                .filter(|v| v.value().id == id && v.value().effective.contains(effective_at)),
        )
    }
}
