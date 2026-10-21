use super::{
    AnalyticsError, AnalyticsInputs, AnnualVolatility, EuropeanEstimate, decimal, identity, model,
};
use crate::ArtifactDigest;
use edgeagent_market_data_domain::OptionPoints;

/// Explicit bisection and identification policy. No automatic bracket expansion.
#[derive(Clone, Copy, PartialEq)]
pub struct IvSolverPolicy {
    lower: AnnualVolatility,
    upper: AnnualVolatility,
    price_tolerance: f64,
    volatility_tolerance: f64,
    minimum_vega: f64,
    iterations: u16,
}
impl IvSolverPolicy {
    /// Require ordered bounds, positive finite tolerances/minimum vega and 1..=256
    /// iterations. Vega uses quotation points per 1.0 fractional volatility.
    /// # Errors
    /// Reject invalid bounds/budget/tolerances rather than selecting defaults.
    pub fn new(
        lower: AnnualVolatility,
        upper: AnnualVolatility,
        price_tolerance: f64,
        volatility_tolerance: f64,
        minimum_vega: f64,
        iterations: u16,
    ) -> Result<Self, AnalyticsError> {
        if lower >= upper
            || !(1..=256).contains(&iterations)
            || [price_tolerance, volatility_tolerance, minimum_vega]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(AnalyticsError::Input);
        }
        Ok(Self {
            lower,
            upper,
            price_tolerance,
            volatility_tolerance,
            minimum_vega,
            iterations,
        })
    }
}
/// Qualified bounded solve, with residual, surviving volatility bracket and work.
#[derive(Clone, Copy, PartialEq)]
pub struct ImpliedVolatility {
    volatility: AnnualVolatility,
    bracket: (f64, f64),
    residual: f64,
    iterations: u16,
    estimate: EuropeanEstimate,
    identity: ArtifactDigest,
}
impl ImpliedVolatility {
    /// Inferred annualized fractional volatility, not a calibrated surface.
    #[must_use]
    pub const fn volatility(self) -> AnnualVolatility {
        self.volatility
    }
    /// Final lower/upper fractional volatility and absolute premium-point residual.
    #[must_use]
    pub const fn convergence(self) -> ((f64, f64), f64, u16) {
        (self.bracket, self.residual, self.iterations)
    }
    /// Greeks evaluated at this result's volatility under identical frozen inputs.
    #[must_use]
    pub const fn estimate(self) -> EuropeanEstimate {
        self.estimate
    }
    /// Input, exact target, solver policy and implementation identity.
    #[must_use]
    pub const fn identity(self) -> ArtifactDigest {
        self.identity
    }
}
macro_rules! redacted {($($t:ty),+)=>{$(impl std::fmt::Debug for $t {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($t),"(..)"))}})+};}
redacted!(IvSolverPolicy, ImpliedVolatility);
pub(super) fn solve(
    input: &AnalyticsInputs,
    premium: OptionPoints,
    policy: IvSolverPolicy,
) -> Result<ImpliedVolatility, AnalyticsError> {
    let target = decimal(premium.decimal_parts());
    let (_, _, lower, upper) = model::bounds(input)?;
    if target <= lower || target >= upper {
        return Err(AnalyticsError::PremiumBounds);
    }
    if target - lower <= policy.price_tolerance || upper - target <= policy.price_tolerance {
        return Err(AnalyticsError::Unidentifiable);
    }
    let (mut lo, mut hi) = (policy.lower.get(), policy.upper.get());
    let lo_price = input.estimate(policy.lower)?.premium_points();
    let hi_price = input.estimate(policy.upper)?.premium_points();
    if lo_price >= hi_price {
        return Err(AnalyticsError::Unidentifiable);
    }
    if target < lo_price || target > hi_price {
        return Err(AnalyticsError::NotBracketed);
    }
    let mut bytes = identity::inputs(input);
    let (coefficient, scale) = premium.decimal_parts();
    bytes.extend(coefficient.to_be_bytes());
    bytes.extend(scale.to_be_bytes());
    for v in [
        lo,
        hi,
        policy.price_tolerance,
        policy.volatility_tolerance,
        policy.minimum_vega,
    ] {
        bytes.extend(v.to_bits().to_be_bytes());
    }
    bytes.extend(policy.iterations.to_be_bytes());
    let identity = ArtifactDigest::of(&bytes);
    for iterations in 1..=policy.iterations {
        let midpoint = lo + (hi - lo) * 0.5;
        if midpoint <= lo || midpoint >= hi {
            return Err(AnalyticsError::NonConvergence);
        }
        let volatility = AnnualVolatility::new(midpoint)?;
        let estimate = input.estimate(volatility)?;
        let residual = (estimate.premium_points() - target).abs();
        // Neither a small price residual in a flat region nor a narrow bracket
        // alone establishes the caller's two convergence requirements.
        if hi - lo <= policy.volatility_tolerance && residual <= policy.price_tolerance {
            if estimate.vega_per_unit_volatility() < policy.minimum_vega {
                return Err(AnalyticsError::Unidentifiable);
            }
            return Ok(ImpliedVolatility {
                volatility,
                bracket: (lo, hi),
                residual,
                iterations,
                estimate,
                identity,
            });
        }
        if estimate.premium_points() <= target {
            lo = midpoint;
        } else {
            hi = midpoint;
        }
    }
    Err(AnalyticsError::NonConvergence)
}
