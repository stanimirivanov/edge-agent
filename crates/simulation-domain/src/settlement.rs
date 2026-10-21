//! Pure whole-position cash settlement, not an account writer or exercise engine.
//! Only explicitly synthetic final fixings are supported. Callers must persist
//! position closure, postings and idempotency together; calculation is not commit.
//!
//! ```
//! use edgeagent_market_data_domain::PreciseInstant;
//! use edgeagent_simulation_domain::{execution::PositionCost, settlement::*};
//! fn defer_without_evidence(policy: &SettlementPolicy, at: PreciseInstant,
//!     position: PositionCost) -> Result<(), SettlementError> {
//!     assert!(matches!(policy.calculate(at, position, 0, &[])?,
//!         SettlementDecision::Deferred(_)));
//!     Ok(())
//! }
//! ```
//!
//! ```compile_fail,E0308
//! use edgeagent_market_data_domain::{IndexPoints, PreciseInstant};
//! use edgeagent_simulation_domain::{execution::PositionCost, settlement::*};
//! fn ordinary_mark_is_not_settlement(policy: &SettlementPolicy,
//!     at: PreciseInstant, position: PositionCost, mark: IndexPoints) {
//!     let _ = policy.calculate(at, position, 0, &[mark]);
//! }
//! ```
mod encoding;

use crate::{ArtifactDigest, execution::PositionCost};
use edgeagent_market_data_domain::{
    IndexPoints, ObservationId, OptionKind, OptionSpecification, PreciseInstant, ReferenceVersion,
    SemanticReference, SourceId,
};

/// Payload-free settlement rejection. No rejection authorizes position closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SettlementError {
    /// American, physical, non-index or otherwise unsupported contract.
    Unsupported,
    /// Explicit fixing/deadline chronology is inconsistent.
    Timing,
    /// Conflicting fixings or evidence belonging to a different policy.
    Evidence,
    /// A whole-position settlement requires a positive authoritative position.
    Position,
    /// Exact arithmetic or the resulting persisted cash exceeds supported bounds.
    Arithmetic,
    /// Explicit fees exceed settlement proceeds plus available unreserved cash.
    Funding,
}
impl std::fmt::Display for SettlementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "settlement rejected: {self:?}")
    }
}
impl std::error::Error for SettlementError {}

/// Explicit rounding applied once to the entire position's USD-cent payoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SettlementRounding {
    /// Discard a fractional cent.
    Down,
    /// Increase any fractional cent to the next cent.
    Up,
    /// Round to nearest cent; exact half-cent ties go to an even cent.
    NearestEven,
}
/// Which contracts incur the supplied per-contract USD-cent charge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FeeBasis {
    /// Charge all contracts, including worthless expirations.
    EveryContract,
    /// Charge only when intrinsic payoff is positive before cent rounding.
    InTheMoney,
}
/// Mandatory fee selection; zero is explicit, not a free-settlement default.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SettlementCharges {
    basis: FeeBasis,
    cents: u64,
}
impl SettlementCharges {
    /// Supply the fee basis and exact USD cents per whole contract.
    #[must_use]
    pub const fn new(basis: FeeBasis, cents_per_contract: u64) -> Self {
        Self {
            basis,
            cents: cents_per_contract,
        }
    }
}

/// Immutable policy with no provider authority or implicit timing/economic rules.
#[derive(Clone, Eq, PartialEq)]
pub struct SettlementPolicy {
    specification: ReferenceVersion<OptionSpecification>,
    semantics: SemanticReference,
    source: SourceId,
    fixing_at: PreciseInstant,
    known_by: PreciseInstant,
    rounding: SettlementRounding,
    charges: SettlementCharges,
}
impl SettlementPolicy {
    /// Pin an exact specification, synthetic source, fixing time, inclusive latest
    /// receipt, rounding and charges. Fixing may differ from contractual expiration.
    ///
    /// # Errors
    /// Reject unsupported products or fixing outside activation..=settlement,
    /// and receipt deadlines preceding the fixing. No calendar/rights are inferred.
    pub fn new(
        specification: ReferenceVersion<OptionSpecification>,
        semantics: SemanticReference,
        source: SourceId,
        fixing_at: PreciseInstant,
        known_by: PreciseInstant,
        rounding: SettlementRounding,
        charges: SettlementCharges,
    ) -> Result<Self, SettlementError> {
        let spec = specification.value();
        spec.validate_initial_simulation_shape()
            .map_err(|_| SettlementError::Unsupported)?;
        if !matches!(spec.terms().payoff().0, OptionKind::Call | OptionKind::Put) {
            return Err(SettlementError::Unsupported);
        }
        let schedule = spec.schedule();
        if fixing_at < PreciseInstant::from_millis(schedule.trading_bounds().0)
            || fixing_at > PreciseInstant::from_millis(schedule.settlement())
            || known_by < fixing_at
        {
            return Err(SettlementError::Timing);
        }
        Ok(Self {
            specification,
            semantics,
            source,
            fixing_at,
            known_by,
            rounding,
            charges,
        })
    }
    /// Versioned canonical artifact. Retain with the run configuration; never use Debug.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encoding::policy(self)
    }
    /// SHA-256 binding for all contractual, provenance and policy choices.
    #[must_use]
    pub fn identity(&self) -> ArtifactDigest {
        ArtifactDigest::of(&self.canonical_bytes())
    }
    /// Calculate without mutation or I/O. Supply the complete candidate fixing set,
    /// authoritative whole-position cost and available **unreserved** cash. Multiple
    /// candidates fail even when equal: this API never silently selects a revision.
    /// Evidence deadline bounds receipt, not age at evaluation: a retained final
    /// fixing does not become a new market quote or expire merely through replay.
    ///
    /// # Errors
    /// Reject zero positions, ambiguous/mismatched evidence, overflow or unfunded
    /// fees. Deferred and error results contain no postings or closure effect.
    pub fn calculate(
        &self,
        at: PreciseInstant,
        position: PositionCost,
        available_cash: u64,
        fixings: &[SyntheticFinalFixing],
    ) -> Result<SettlementDecision, SettlementError> {
        let (units, cost) = position.values();
        if units == 0 {
            return Err(SettlementError::Position);
        }
        if fixings.len() > 1 {
            return Err(SettlementError::Evidence);
        }
        let spec = self.specification.value();
        if at < PreciseInstant::from_millis(spec.schedule().settlement()) {
            return Ok(SettlementDecision::Deferred(DeferredReason::NotDue));
        }
        let [fixing] = fixings else {
            return Ok(SettlementDecision::Deferred(
                DeferredReason::MissingEvidence,
            ));
        };
        let policy = self.identity();
        if fixing.policy != policy {
            return Err(SettlementError::Evidence);
        }
        if at < fixing.known_at || at < PreciseInstant::from_millis(self.specification.known_at()) {
            return Ok(SettlementDecision::Deferred(DeferredReason::NotKnown));
        }
        let (kind, strike) = spec.terms().payoff();
        let strike = scaled(strike.decimal_parts())?;
        let level = scaled(fixing.level.decimal_parts())?;
        let intrinsic = match kind {
            OptionKind::Call => level.saturating_sub(strike),
            OptionKind::Put => strike.saturating_sub(level),
            _ => return Err(SettlementError::Unsupported),
        };
        // Points and USD/point each have eight decimal places. Their product
        // is 10^-14 USD cents; keep it exact and round once over the whole lot.
        let exact = intrinsic
            .checked_mul(scaled(spec.terms().quotation().1.decimal_parts())?)
            .and_then(|n| n.checked_mul(u128::from(units)))
            .ok_or(SettlementError::Arithmetic)?;
        let gross = round(exact, self.rounding)?;
        let charged = match self.charges.basis {
            FeeBasis::EveryContract => true,
            FeeBasis::InTheMoney => intrinsic > 0,
        };
        let fees = if charged {
            self.charges
                .cents
                .checked_mul(units)
                .ok_or(SettlementError::Arithmetic)?
        } else {
            0
        };
        if fees > gross && fees - gross > available_cash {
            return Err(SettlementError::Funding);
        }
        // Subtract before addition so a large gross offset by fees need not
        // overflow an otherwise representable final available-cash balance.
        let cash_after = if gross >= fees {
            available_cash.checked_add(gross - fees)
        } else {
            available_cash.checked_sub(fees - gross)
        }
        .ok_or(SettlementError::Arithmetic)?;
        Ok(SettlementDecision::Calculated(SettlementCalculation {
            policy,
            evidence: fixing.identity(),
            units,
            gross,
            fees,
            cash_after,
            postings: [
                i128::from(gross) - i128::from(fees),
                -i128::from(cost),
                i128::from(cost) - i128::from(gross),
                i128::from(fees),
            ],
        }))
    }
}

/// Invented final settlement observation, deliberately not an ordinary index mark.
/// No authoritative-provider constructor, correction selection or wire decoder is
/// provided. The policy binding includes the exact specification and source.
#[derive(Clone, Eq, PartialEq)]
pub struct SyntheticFinalFixing {
    policy: ArtifactDigest,
    id: ObservationId,
    known_at: PreciseInstant,
    level: IndexPoints,
}
impl SyntheticFinalFixing {
    /// Declare one final invented fixing and its actual knowledge instant.
    /// Finality here is a synthetic scenario assumption, not provider certification.
    ///
    /// # Errors
    /// Returns `Timing` unless fixing <= receipt <= the policy's receipt deadline.
    pub fn new(
        policy: &SettlementPolicy,
        id: ObservationId,
        known_at: PreciseInstant,
        level: IndexPoints,
    ) -> Result<Self, SettlementError> {
        if known_at < policy.fixing_at || known_at > policy.known_by {
            return Err(SettlementError::Timing);
        }
        Ok(Self {
            policy: policy.identity(),
            id,
            known_at,
            level,
        })
    }
    /// Exact synthetic artifact bytes, separate from any provider's payload.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encoding::fixing(self)
    }
    /// Digest includes synthetic marker, policy, identity, level and knowledge time.
    #[must_use]
    pub fn identity(&self) -> ArtifactDigest {
        ArtifactDigest::of(&self.canonical_bytes())
    }
}
/// No economic effect exists until the evidence and timing gates pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DeferredReason {
    /// Contractual settlement has not arrived; order expiry is not position closure.
    NotDue,
    /// No final settlement evidence is available. This is never a zero payoff.
    MissingEvidence,
    /// The fixing or specification was not known at the injected evaluation time.
    NotKnown,
}
/// Pure result. A calculated value is not a durable settlement receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SettlementDecision {
    /// Keep exposure and funding unchanged.
    Deferred(DeferredReason),
    /// Candidate effects for one atomic, idempotent account transaction.
    Calculated(SettlementCalculation),
}
/// Balanced candidate effects using P09's cash/cost/realized-expense/fee convention.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SettlementCalculation {
    policy: ArtifactDigest,
    evidence: ArtifactDigest,
    units: u64,
    gross: u64,
    fees: u64,
    cash_after: u64,
    postings: [i128; 4],
}
impl SettlementCalculation {
    /// Whole contracts closed, gross USD cents and separate fee cents.
    #[must_use]
    pub const fn amounts(self) -> (u64, u64, u64) {
        (self.units, self.gross, self.fees)
    }
    /// Remaining available cash and balanced signed postings. The supplied position
    /// closes completely, releasing all its cost; other positions remain untouched.
    #[must_use]
    pub const fn effects(self) -> (u64, [i128; 4]) {
        (self.cash_after, self.postings)
    }
    /// Exact policy and fixing artifact identities to retain with the account receipt.
    #[must_use]
    pub const fn provenance(self) -> (ArtifactDigest, ArtifactDigest) {
        (self.policy, self.evidence)
    }
}
macro_rules! redacted {
    ($($t:ty),+ $(,)?) => {$(impl std::fmt::Debug for $t {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(concat!(stringify!($t), "(..)"))
        }
    })+};
}
redacted!(
    SettlementCharges,
    SettlementPolicy,
    SyntheticFinalFixing,
    SettlementCalculation
);

fn scaled((coefficient, scale): (i128, u32)) -> Result<u128, SettlementError> {
    u128::try_from(coefficient)
        .ok()
        .and_then(|n| n.checked_mul(10_u128.checked_pow(8_u32.checked_sub(scale)?)?))
        .ok_or(SettlementError::Arithmetic)
}
fn round(exact: u128, rounding: SettlementRounding) -> Result<u64, SettlementError> {
    const DENOMINATOR: u128 = 100_000_000_000_000;
    let whole = exact / DENOMINATOR;
    let remainder = exact % DENOMINATOR;
    let add = match rounding {
        SettlementRounding::Down => false,
        SettlementRounding::Up => remainder != 0,
        SettlementRounding::NearestEven => {
            remainder * 2 > DENOMINATOR
                || (remainder * 2 == DENOMINATOR && !whole.is_multiple_of(2))
        }
    };
    u64::try_from(whole + u128::from(add)).map_err(|_| SettlementError::Arithmetic)
}
