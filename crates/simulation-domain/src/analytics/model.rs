use super::{AnalyticsError, AnalyticsInputs, AnnualVolatility, identity};
use crate::ArtifactDigest;
use edgeagent_market_data_domain::OptionKind;

/// Finite binary64 theoretical premium/Greeks, never exact money or executable evidence.
#[derive(Clone, Copy, PartialEq)]
pub struct EuropeanEstimate {
    values: [f64; 6],
    identity: ArtifactDigest,
}
impl EuropeanEstimate {
    /// Unrounded option quotation points, before multiplier and contract quantity.
    #[must_use]
    pub const fn premium_points(self) -> f64 {
        self.values[0]
    }
    /// Premium-point change per one underlying index point.
    #[must_use]
    pub const fn delta(self) -> f64 {
        self.values[1]
    }
    /// Delta change per one underlying index point.
    #[must_use]
    pub const fn gamma_per_point(self) -> f64 {
        self.values[2]
    }
    /// Premium-point change per 1.0 annualized volatility (divide by 100 for 1%).
    #[must_use]
    pub const fn vega_per_unit_volatility(self) -> f64 {
        self.values[3]
    }
    /// Premium-point change per elapsed year, holding rates/yield/volatility fixed.
    #[must_use]
    pub const fn theta_per_year(self) -> f64 {
        self.values[4]
    }
    /// Premium-point change per 1.0 continuously compounded risk-free rate.
    #[must_use]
    pub const fn rho_per_unit_rate(self) -> f64 {
        self.values[5]
    }
    /// Immutable input plus model/volatility identity; not a source attestation.
    #[must_use]
    pub const fn identity(self) -> ArtifactDigest {
        self.identity
    }
}
impl std::fmt::Debug for EuropeanEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EuropeanEstimate(..)")
    }
}
pub(super) fn bounds(input: &AnalyticsInputs) -> Result<(f64, f64, f64, f64), AnalyticsError> {
    let a = input.spot * libm::exp(-input.rates.yield_rate * input.years);
    let b = input.strike * libm::exp(-input.rates.risk_free * input.years);
    let (lower, upper) = match input.kind {
        OptionKind::Call => ((a - b).max(0.0), a),
        OptionKind::Put => ((b - a).max(0.0), b),
        _ => return Err(AnalyticsError::Unsupported),
    };
    if ![a, b, lower, upper].iter().all(|n| n.is_finite()) || upper <= lower {
        return Err(AnalyticsError::Numerical);
    }
    Ok((a, b, lower, upper))
}
fn cdf(d: f64) -> f64 {
    0.5 * libm::erfc(-d / std::f64::consts::SQRT_2)
}
pub(super) fn evaluate(
    input: &AnalyticsInputs,
    vol: AnnualVolatility,
) -> Result<EuropeanEstimate, AnalyticsError> {
    let (a, b, lower, upper) = bounds(input)?;
    let t = input.years;
    let root = libm::sqrt(t);
    let sigma = vol.get();
    let width = sigma * root;
    let d1 = (libm::log(input.spot / input.strike)
        + (input.rates.risk_free - input.rates.yield_rate + 0.5 * sigma * sigma) * t)
        / width;
    let d2 = d1 - width;
    if width <= 0.0 || ![width, d1, d2].iter().all(|v| v.is_finite()) {
        return Err(AnalyticsError::Numerical);
    }
    let (n1, n2, m1, m2) = (cdf(d1), cdf(d2), cdf(-d1), cdf(-d2));
    let density = libm::exp(-0.5 * d1 * d1) / libm::sqrt(2.0 * std::f64::consts::PI);
    // Price the out-of-money leg directly, then apply parity for the other leg.
    // erfc evaluates the tail directly; never subtract its probability from 1.
    let (call, put) = if a <= b {
        let c = a * n1 - b * n2;
        (c, c + (b - a))
    } else {
        let p = b * m2 - a * m1;
        (p + (a - b), p)
    };
    let discount = libm::exp(-input.rates.yield_rate * t);
    let common = -a * density * sigma / (2.0 * root);
    let (premium, delta, theta, rho) = match input.kind {
        OptionKind::Call => (
            call,
            discount * n1,
            common - input.rates.risk_free * b * n2 + input.rates.yield_rate * a * n1,
            t * b * n2,
        ),
        OptionKind::Put => (
            put,
            -discount * m1,
            common + input.rates.risk_free * b * m2 - input.rates.yield_rate * a * m1,
            -t * b * m2,
        ),
        _ => return Err(AnalyticsError::Unsupported),
    };
    let values = [
        premium,
        delta,
        discount * density / (input.spot * width),
        a * density * root,
        theta,
        rho,
    ];
    // A finite zero at a theoretical open bound is still lost time value, not a
    // qualified exact price. Fail rather than clamp away underflow/cancellation.
    if values.iter().any(|v| !v.is_finite()) || premium <= lower || premium >= upper {
        return Err(AnalyticsError::Numerical);
    }
    let mut bytes = identity::inputs(input);
    bytes.extend(vol.get().to_bits().to_be_bytes());
    Ok(EuropeanEstimate {
        values,
        identity: ArtifactDigest::of(&bytes),
    })
}
