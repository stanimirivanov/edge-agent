//! Theoretical BSM analytics, never executable prices, ledger money or fixings.
//! Rates, year basis, evidence and IV tolerances are explicit. Statistical f64
//! outputs require tolerance-based replay; they cannot authorize a financial effect.
//!
//! ```
//! use edgeagent_simulation_domain::analytics::AnnualVolatility;
//! assert!(AnnualVolatility::new(f64::NAN).is_err());
//! assert!(AnnualVolatility::new(0.0).is_err());
//! let volatility = AnnualVolatility::new(0.2)?;
//! assert_eq!(volatility.get(), 0.2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Theory cannot be supplied as canonical executable premium evidence:
//! ```compile_fail,E0308
//! use edgeagent_market_data_domain::OptionPoints;
//! use edgeagent_simulation_domain::analytics::EuropeanEstimate;
//! fn executable_premium(_: OptionPoints) {}
//! fn invalid(estimate: EuropeanEstimate) { executable_premium(estimate); }
//! ```
mod identity;
mod model;
mod solver;
use crate::ArtifactDigest;
use edgeagent_market_data_domain::{
    IndexPoints, OptionKind, OptionSpecification, PreciseInstant, ReferenceVersion,
};
pub use model::EuropeanEstimate;
pub use solver::{ImpliedVolatility, IvSolverPolicy};

/// Payload-free numerical/qualification rejection; never contains supplied values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnalyticsError {
    /// Nonfinite, degenerate or outside the documented computational envelope.
    Input,
    /// Not the supported cash-settled European index-option shape.
    Unsupported,
    /// Specification is not effective or not yet known at injected evaluation time.
    Reference,
    /// No positive remaining lifetime; expiry payoff belongs to settlement.
    Expired,
    /// Arithmetic overflow, underflow or loss of identifiable time value.
    Numerical,
    /// Target is outside strict discounted no-arbitrage bounds.
    PremiumBounds,
    /// Supplied volatility endpoints do not enclose the target premium.
    NotBracketed,
    /// Time value or vega is too small for the supplied identification policy.
    Unidentifiable,
    /// Both convergence criteria were not met within the explicit work bound.
    NonConvergence,
}
impl std::fmt::Display for AnalyticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "option analytics rejected: {self:?}")
    }
}
impl std::error::Error for AnalyticsError {}

/// Explicit year-fraction convention; no trading-day or calendar inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum YearBasis {
    /// Actual elapsed UTC seconds divided by 365 * 86400, including subsecond time.
    Actual365Fixed,
}
/// Continuously compounded annual rates in fractional units, not percentages.
#[derive(Clone, Copy, PartialEq)]
pub struct CarryRates {
    risk_free: f64,
    yield_rate: f64,
    assumptions: ArtifactDigest,
}
impl CarryRates {
    /// Bind explicit finite rates in [-1,1] and retained assumption provenance.
    /// These are numerical envelope bounds, not recommended or authorized rates.
    /// # Errors
    /// Reject nonfinite or out-of-envelope rates. Negative rates/yields are supported.
    pub fn new(
        risk_free: f64,
        yield_rate: f64,
        assumptions: ArtifactDigest,
    ) -> Result<Self, AnalyticsError> {
        if [risk_free, yield_rate]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1.0)
        {
            return Err(AnalyticsError::Input);
        }
        Ok(Self {
            risk_free: zero(risk_free),
            yield_rate: zero(yield_rate),
            assumptions,
        })
    }
}
/// Positive annualized fractional volatility, bounded to 10 for this implementation.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct AnnualVolatility(f64);
impl AnnualVolatility {
    /// No default and no zero-volatility derivative convention.
    /// # Errors
    /// Reject nonfinite, zero, negative or greater-than-10 input.
    pub fn new(value: f64) -> Result<Self, AnalyticsError> {
        if !value.is_finite() || value <= 0.0 || value > 10.0 {
            return Err(AnalyticsError::Input);
        }
        Ok(Self(value))
    }
    /// Annualized fractional volatility; 0.2 means 20%, not 0.2%.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}
/// Frozen numerical inputs. Caller retains/qualifies the evidence identified by
/// the digest; this constructor cannot prove freshness, rights or source authority.
#[derive(Clone)]
pub struct AnalyticsInputs {
    reference: ReferenceVersion<OptionSpecification>,
    at: PreciseInstant,
    level: IndexPoints,
    rates: CarryRates,
    basis: YearBasis,
    evidence: ArtifactDigest,
    spot: f64,
    strike: f64,
    years: f64,
    kind: OptionKind,
}
impl AnalyticsInputs {
    /// Validate supported shape, effective/knowledge time and strictly positive
    /// remaining ACT/365F lifetime <=100 years. Spot/strike must be <=1e12 points.
    /// Multiplier and tick remain reference metadata: estimates are per-point
    /// continuous values, not tick-rounded cash or a contract-position valuation.
    /// # Errors
    /// Reject unsupported, expired, future-known/effectively invalid references or
    /// numeric values outside the computational envelope. No wall clock is read.
    pub fn new(
        reference: ReferenceVersion<OptionSpecification>,
        at: PreciseInstant,
        level: IndexPoints,
        rates: CarryRates,
        basis: YearBasis,
        evidence: ArtifactDigest,
    ) -> Result<Self, AnalyticsError> {
        let spec = reference.value();
        spec.validate_initial_simulation_shape()
            .map_err(|_| AnalyticsError::Unsupported)?;
        let expiry = PreciseInstant::from_millis(spec.schedule().expiration());
        if at >= expiry {
            return Err(AnalyticsError::Expired);
        }
        let (start, end) = spec.effective().bounds();
        if at < PreciseInstant::from_millis(reference.known_at())
            || at < PreciseInstant::from_millis(start)
            || at >= PreciseInstant::from_millis(end)
            || at < PreciseInstant::from_millis(spec.schedule().trading_bounds().0)
        {
            return Err(AnalyticsError::Reference);
        }
        let (s, n) = at.unix_parts();
        let (es, en) = expiry.unix_parts();
        let nanos =
            (i128::from(es) - i128::from(s)) * 1_000_000_000 + i128::from(en) - i128::from(n);
        let years = nanos as f64 / (365.0 * 86400.0 * 1e9);
        let (kind, strike) = spec.terms().payoff();
        let spot = decimal(level.decimal_parts());
        let strike = decimal(strike.decimal_parts());
        if years <= 0.0 || years > 100.0 || spot > 1e12 || strike > 1e12 {
            return Err(AnalyticsError::Input);
        }
        Ok(Self {
            reference,
            at,
            level,
            rates,
            basis,
            evidence,
            spot,
            strike,
            years,
            kind,
        })
    }
    /// Canonical complete-input identity, including implementation and exact decimals.
    #[must_use]
    pub fn identity(&self) -> ArtifactDigest {
        ArtifactDigest::of(&identity::inputs(self))
    }
    /// Theoretical quotation points and sensitivities, with no monetary side effects.
    /// # Errors
    /// Reject nonfinite/degenerate intermediate results or unresolvable time value.
    pub fn estimate(
        &self,
        volatility: AnnualVolatility,
    ) -> Result<EuropeanEstimate, AnalyticsError> {
        model::evaluate(self, volatility)
    }
    /// Infer volatility only under the supplied explicit bounded policy.
    /// # Errors
    /// Reject invalid premium, no bracket, low identification or non-convergence.
    pub fn implied_volatility(
        &self,
        premium: edgeagent_market_data_domain::OptionPoints,
        policy: IvSolverPolicy,
    ) -> Result<ImpliedVolatility, AnalyticsError> {
        solver::solve(self, premium, policy)
    }
}
fn decimal((n, scale): (i128, u32)) -> f64 {
    n as f64 / 10_f64.powi(scale as i32)
}
fn zero(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { v }
}
macro_rules! redacted {($($t:ty),+)=>{$(impl std::fmt::Debug for $t {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($t),"(..)"))}})+};}
redacted!(CarryRates, AnnualVolatility, AnalyticsInputs);
