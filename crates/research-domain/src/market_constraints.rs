//! Exact population volatility and opening gaps over the selected raw-price census.
use crate::{EligibleSeries, momentum};
use edgeagent_market_data_domain::{InstrumentId, SessionDate, UsdPrice};
/// Population SD of ties-even interval ppm returns, ceiling to ppm, without annualization.
pub const VOLATILITY_REVISION: &str = "population-sd-rounded-return-ceil-ppm-v1";
/// Maximum absolute opening return against selected prior close, ceiling to ppm.
pub const GAP_REVISION: &str = "max-selected-opening-gap-ceil-ppm-v1";
/// Owner-bound daily opening assertion.
pub struct Open {
    /// Exact selected session.
    pub session: SessionDate,
    /// Raw USD per share.
    pub price: UsdPrice,
    /// Any owner-reported action in this selected bucket; unsupported here.
    pub action: bool,
}
/// Opening census from the same retained object as eligible closes.
pub struct Input {
    /// Stable instrument.
    pub instrument: InstrumentId,
    /// Exact retained daily digest.
    pub evidence: [u8; 32],
    /// Complete selected sessions, including the initial close anchor.
    pub rows: Vec<Open>,
}
/// Safe numerical or evidence refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Missing session or predecessor.
    InsufficientHistory,
    /// Wrong binding, duplicate/foreign session or unsafe action.
    InvalidEvidence,
    /// Interval i64 ppm return or final u32 metric cannot be represented.
    OutOfRange,
    /// Preserved failure from the existing interval-return implementation.
    Return(momentum::MomentumError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("market constraint calculation refused")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Return(e) => Some(e),
            _ => None,
        }
    }
}
/// Sealed metrics, not an eligibility or publication assertion.
pub struct Metrics {
    instrument: InstrumentId,
    volatility: u32,
    gap: u32,
}
impl Metrics {
    /// Bound instrument.
    #[must_use]
    pub const fn instrument(&self) -> InstrumentId {
        self.instrument
    }
    /// Ceiling population standard deviation of rounded interval returns, ppm.
    #[must_use]
    pub const fn volatility_ppm(&self) -> u32 {
        self.volatility
    }
    /// Ceiling maximum absolute opening gap, ppm.
    #[must_use]
    pub const fn gap_ppm(&self) -> u32 {
        self.gap
    }
}
fn gap(first: UsdPrice, open: UsdPrice) -> Result<u32, Error> {
    let denominator = momentum::coefficient(first).map_err(Error::Return)?;
    let delta = (momentum::coefficient(open).map_err(Error::Return)? - denominator).abs();
    let mut q = delta / denominator;
    let mut r = delta % denominator;
    for _ in 0..6 {
        r = r.checked_mul(10).ok_or(Error::OutOfRange)?;
        q = q
            .checked_mul(10)
            .and_then(|v| v.checked_add(r / denominator))
            .ok_or(Error::OutOfRange)?;
        r %= denominator;
    }
    u32::try_from(q.checked_add(i128::from(r > 0)).ok_or(Error::OutOfRange)?)
        .map_err(|_| Error::OutOfRange)
}
fn volatility(returns: &[i64]) -> Result<u32, Error> {
    // Population variance = sum_{i<j}(r_i-r_j)^2 / N^2, avoiding cancellation.
    let n = returns.len() as u128;
    let mut squares = 0_u128;
    for (i, a) in returns.iter().enumerate() {
        for b in &returns[..i] {
            let d = (i128::from(*a) - i128::from(*b)).unsigned_abs();
            squares = squares
                .checked_add(d.checked_mul(d).ok_or(Error::OutOfRange)?)
                .ok_or(Error::OutOfRange)?;
        }
    }
    let bound = u128::from(u32::MAX) * n;
    if squares > bound * bound {
        return Err(Error::OutOfRange);
    }
    let (mut lo, mut hi) = (0_u32, u32::MAX);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let v = u128::from(mid) * n;
        if v * v >= squares {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(lo)
}
/// Compute both explicit revisions with exact selected-session and daily-digest binding.
/// # Errors
/// Missing history abstains; malformed/action-affected inputs reject; overflow never clamps.
pub fn calculate(series: &EligibleSeries<'_>, input: &Input) -> Result<Metrics, Error> {
    let closes = series.closes();
    if input.instrument != series.frozen().selected.instrument
        || input.evidence != series.frozen().evidence
        || input.rows.len() > 253
    {
        return Err(Error::InvalidEvidence);
    }
    let mut rows = input.rows.iter().collect::<Vec<_>>();
    rows.sort_by_key(|r| r.session);
    if rows.windows(2).any(|w| w[0].session == w[1].session)
        || rows
            .iter()
            .any(|r| r.action || !closes.iter().any(|c| c.session == r.session))
    {
        return Err(Error::InvalidEvidence);
    }
    if rows.len() != closes.len() {
        return Err(Error::InsufficientHistory);
    }
    let mut returns = Vec::with_capacity(closes.len() - 1);
    let mut maximum = 0;
    for (i, pair) in closes.windows(2).enumerate() {
        returns.push(momentum::score(pair[0].close, pair[1].close).map_err(Error::Return)?);
        maximum = maximum.max(gap(pair[0].close, rows[i + 1].price)?);
    }
    Ok(Metrics {
        instrument: input.instrument,
        volatility: volatility(&returns)?,
        gap: maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn population_and_gap_vectors_include_ceilings_and_range_limits() -> Result {
        assert_eq!(volatility(&[100_000, -100_000])?, 100_000);
        assert_eq!(volatility(&[0, 1])?, 1);
        assert_eq!(volatility(&[42])?, 0);
        assert!(
            std::error::Error::source(&Error::Return(momentum::MomentumError::OutOfRange))
                .is_some()
        );
        assert_eq!(volatility(&[0, 2 * i64::from(u32::MAX)])?, u32::MAX);
        assert_eq!(
            volatility(&[0, 2 * i64::from(u32::MAX) + 1]).err(),
            Some(Error::OutOfRange)
        );
        assert_eq!(volatility(&[i64::MAX; i64::BITS as usize])?, 0);
        assert_eq!(volatility(&[0, i64::MAX]).err(), Some(Error::OutOfRange));
        assert_eq!(
            gap(UsdPrice::new(2_000_000, 0)?, UsdPrice::new(2_000_001, 0)?)?,
            1
        );
        assert_eq!(
            gap(UsdPrice::new(2_000_000, 0)?, UsdPrice::new(1_999_999, 0)?)?,
            1
        );
        assert_eq!(
            gap(
                UsdPrice::new(1_000_000, 0)?,
                UsdPrice::new(i128::from(u32::MAX) + 1_000_000, 0)?
            )?,
            u32::MAX
        );
        assert_eq!(
            gap(
                UsdPrice::new(1_000_000, 0)?,
                UsdPrice::new(i128::from(u32::MAX) + 1_000_001, 0)?
            )
            .err(),
            Some(Error::OutOfRange)
        );
        assert_eq!(
            gap(
                UsdPrice::new(10_i128.pow(28) - 1, 0)?,
                UsdPrice::new(10_i128.pow(28) - 2, 0)?
            )?,
            1
        );
        assert_eq!(
            VOLATILITY_REVISION,
            "population-sd-rounded-return-ceil-ppm-v1"
        );
        assert_eq!(GAP_REVISION, "max-selected-opening-gap-ceil-ppm-v1");
        Ok(())
    }
    #[test]
    fn independent_small_integer_oracles_agree() -> Result {
        for a in -3_i64..=3 {
            for b in -3_i64..=3 {
                for c in -3_i64..=3 {
                    let numerator = 3 * (a * a + b * b + c * c) - (a + b + c) * (a + b + c);
                    let expected = (0_u32..=6)
                        .find(|k| 9 * i64::from(*k) * i64::from(*k) >= numerator)
                        .ok_or("oracle")?;
                    assert_eq!(volatility(&[a, b, c])?, expected);
                }
            }
        }
        for first in 1_i128..=20 {
            for open in 1_i128..=20 {
                let numerator = (open - first).abs() * 1_000_000;
                assert_eq!(
                    gap(UsdPrice::new(first, 0)?, UsdPrice::new(open, 0)?)?,
                    u32::try_from((numerator + first - 1) / first)?
                );
            }
        }
        Ok(())
    }
}
