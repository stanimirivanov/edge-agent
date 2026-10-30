//! Exact close-volume proxy; not actual traded notional or executable liquidity.
use crate::EligibleSeries;
use edgeagent_market_data_domain::{InstrumentId, SessionDate, ShareVolume};
/// Shared lookback+1 window, raw USD close × shares, mean then one floor to cents.
pub const REVISION: &str = "mean-selected-close-volume-floor-cents-v1";
/// Owner-authorized reported share volume; zero is observed, absence is not zero.
pub struct Volume {
    /// Explicit qualified session.
    pub session: SessionDate,
    /// Exact reported shares.
    pub shares: ShareVolume,
}
/// Volume assertions from the same retained daily artifact as the eligible closes.
pub struct Input {
    /// Exact instrument identity.
    pub instrument: InstrumentId,
    /// Retained daily artifact digest, not independent authority.
    pub evidence: [u8; 32],
    /// Complete selected-session census.
    pub rows: Vec<Volume>,
}
/// Safe calculation refusal, never raw evidence values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Required selected session is missing.
    InsufficientHistory,
    /// Binding, duplicate or foreign session is invalid.
    InvalidEvidence,
    /// Exact arithmetic or the u64 cents result cannot be represented.
    OutOfRange,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("liquidity calculation refused")
    }
}
impl std::error::Error for Error {}
/// Sealed metric, computed only from bound complete inputs.
pub struct Liquidity {
    instrument: InstrumentId,
    cents: u64,
}
impl Liquidity {
    /// Instrument bound during calculation.
    #[must_use]
    pub const fn instrument(&self) -> InstrumentId {
        self.instrument
    }
    /// Floor of the exact mean proxy in whole USD cents.
    #[must_use]
    pub const fn cents(&self) -> u64 {
        self.cents
    }
}
/// Calculate exact selected-session mean with no per-row truncation or floating point.
/// # Errors
/// Rejects missing/foreign/duplicate rows, changed evidence identity and numeric overflow.
pub fn calculate(series: &EligibleSeries<'_>, input: &Input) -> Result<Liquidity, Error> {
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
            .any(|r| !closes.iter().any(|c| c.session == r.session))
    {
        return Err(Error::InvalidEvidence);
    }
    if rows.len() != closes.len() {
        return Err(Error::InsufficientHistory);
    }
    let mut sum = 0_i128;
    for (close, row) in closes.iter().zip(rows) {
        let (p, ps) = close.close.decimal_parts();
        let (v, vs) = row.shares.decimal_parts();
        let price = p
            .checked_mul(10_i128.pow(8 - ps))
            .ok_or(Error::OutOfRange)?;
        let volume = v
            .checked_mul(10_i128.pow(6 - vs))
            .ok_or(Error::OutOfRange)?;
        sum = sum
            .checked_add(price.checked_mul(volume).ok_or(Error::OutOfRange)?)
            .ok_or(Error::OutOfRange)?;
    }
    // Fixed scale 14 -> cents scale 2; divide only once, after the complete exact sum.
    let denominator =
        i128::try_from(closes.len()).map_err(|_| Error::OutOfRange)? * 1_000_000_000_000;
    let cents = u64::try_from(sum / denominator).map_err(|_| Error::OutOfRange)?;
    Ok(Liquidity {
        instrument: input.instrument,
        cents,
    })
}
