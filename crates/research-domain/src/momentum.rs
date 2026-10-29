//! Versioned raw USD endpoint rate of change, without floating point or I/O.
use crate::{AdjustmentBasis, EligibleSeries};
use edgeagent_market_data_domain::{SessionDate, UsdPrice};

/// Immutable formula, units, rounding and endpoint-selection revision.
pub const REVISION: &str = "raw-usd-roc-ppm-half-even-v1";
/// Dimensionless score scale: one ppm is 0.000001 return, not one percent.
pub const PPM_PER_RETURN: i128 = 1_000_000;
/// Maximum rounding error is this many half-ppm units (exactly 0.5 ppm).
pub const MAX_ERROR_HALF_PPM: u8 = 1;
/// Payload-safe calculation failure; no price values or retained metadata are displayed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MomentumError {
    /// Eligible evidence unexpectedly lacks its complete endpoints or raw basis.
    InvalidEvidence,
    /// Checked arithmetic or the signed 64-bit wire score cannot represent the result.
    OutOfRange,
}
impl std::fmt::Display for MomentumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEvidence => "invalid momentum evidence",
            Self::OutOfRange => "momentum score out of range",
        })
    }
}
impl std::error::Error for MomentumError {}
/// Rounded score with explicit lookback, units/basis revision and endpoint sessions.
/// Evidence and configuration identities remain in the borrowed eligible source.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Momentum {
    score_ppm: i64,
    lookback_sessions: u16,
    first_session: SessionDate,
    last_session: SessionDate,
}
impl Momentum {
    /// Dimensionless integer parts per million; negative returns remain negative.
    #[must_use]
    pub const fn score_ppm(self) -> i64 {
        self.score_ppm
    }
    /// Number of selected trading-session intervals, not calendar days or holding horizon.
    #[must_use]
    pub const fn lookback_sessions(self) -> u16 {
        self.lookback_sessions
    }
    /// Exact oldest and latest selected sessions, respectively.
    #[must_use]
    pub const fn endpoint_sessions(self) -> (SessionDate, SessionDate) {
        (self.first_session, self.last_session)
    }
    /// Qualified price basis for this revision.
    #[must_use]
    pub const fn basis(self) -> AdjustmentBasis {
        AdjustmentBasis::Raw
    }
    /// Version binding formula, ppm units, ties-to-even rounding and half-ppm tolerance.
    #[must_use]
    pub const fn revision(self) -> &'static str {
        REVISION
    }
}
fn coefficient(price: UsdPrice) -> Result<i128, MomentumError> {
    let (value, scale) = price.decimal_parts();
    // UsdPrice supports at most eight fractional places and 28 normalized digits.
    // Align to that existing exact representation, without changing price semantics.
    let exponent = 8_u32
        .checked_sub(scale)
        .ok_or(MomentumError::InvalidEvidence)?;
    value
        .checked_mul(10_i128.pow(exponent))
        .filter(|v| *v > 0)
        .ok_or(MomentumError::OutOfRange)
}
fn score(first: UsdPrice, last: UsdPrice) -> Result<i64, MomentumError> {
    let denominator = coefficient(first)?;
    let delta = coefficient(last)?
        .checked_sub(denominator)
        .ok_or(MomentumError::OutOfRange)?;
    let negative = delta < 0;
    let magnitude = delta.checked_abs().ok_or(MomentumError::OutOfRange)?;
    let mut quotient = magnitude / denominator;
    let mut remainder = magnitude % denominator;
    // Six exact decimal digits. Each remainder is below denominator, so multiplying
    // it by ten fits the existing USD bound even when delta*1_000_000 would not.
    for _ in 0..6 {
        remainder = remainder.checked_mul(10).ok_or(MomentumError::OutOfRange)?;
        quotient = quotient
            .checked_mul(10)
            .and_then(|v| v.checked_add(remainder / denominator))
            .ok_or(MomentumError::OutOfRange)?;
        remainder %= denominator;
    }
    // Compare to half without multiplying a large remainder by two.
    let half = remainder.cmp(&(denominator - remainder));
    if half.is_gt() || (half.is_eq() && quotient % 2 != 0) {
        quotient = quotient.checked_add(1).ok_or(MomentumError::OutOfRange)?;
    }
    let signed = if negative {
        quotient.checked_neg().ok_or(MomentumError::OutOfRange)?
    } else {
        quotient
    };
    i64::try_from(signed).map_err(|_| MomentumError::OutOfRange)
}
/// Compute `(last / first - 1) * 1_000_000`, nearest integer, ties to even.
/// Only complete sealed raw-USD evidence is accepted; no alternate basis is inferred.
/// No intermediate rounding occurs. Successful output differs from the exact score
/// by at most 0.5 ppm; an unrepresentable score is never clamped or saturated.
/// # Errors
/// Returns a payload-safe error for invalid sealed endpoints or checked numeric overflow.
pub fn calculate(series: &EligibleSeries<'_>) -> Result<Momentum, MomentumError> {
    let closes = series.closes();
    let first = closes.first().ok_or(MomentumError::InvalidEvidence)?;
    let last = closes.last().ok_or(MomentumError::InvalidEvidence)?;
    let lookback_sessions = closes
        .len()
        .checked_sub(1)
        .and_then(|v| u16::try_from(v).ok())
        .filter(|v| (1..=252).contains(v))
        .ok_or(MomentumError::InvalidEvidence)?;
    if series.frozen().basis != AdjustmentBasis::Raw {
        return Err(MomentumError::InvalidEvidence);
    }
    Ok(Momentum {
        score_ppm: score(first.close, last.close)?,
        lookback_sessions,
        first_session: first.session,
        last_session: last.session,
    })
}
