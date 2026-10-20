//! Explicit, conservative execution economics. No vendor, clock, transport or I/O.
//! A proposal consumes no liquidity until its owner commits all account effects.

mod accounting;
mod evidence;

pub use accounting::{FillAccounting, PositionCost};
pub use evidence::ExecutableLiquidity;
pub use evidence::LiquidityExecution;

use crate::{ArtifactDigest, orders::OrderError};
use edgeagent_market_data_domain::{PreciseInstant, SemanticReference};
use std::time::Duration;

/// Versioned fee/participation/latency policy. All values are mandatory, including
/// explicit zeros. Fee units are millionths of one USD cent, not dollar micros.
/// Fixed fees are charged once, on the first actual fill; no fee for no-fill.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ExecutionProfile {
    version: SemanticReference,
    participation: u16,
    fixed_fee: u64,
    unit_fee: u64,
    submission: Duration,
    cancellation: Duration,
    digest: ArtifactDigest,
}
impl ExecutionProfile {
    /// Pin the complete deterministic model configuration. Zero latency/fees are
    /// valid only when explicitly supplied. Participation is 1..=10000 bps.
    ///
    /// # Errors
    /// Returns `ModelUnavailable` for zero/excess participation or latency >1 day.
    pub fn new(
        version: SemanticReference,
        participation_bps: u16,
        fixed_fee_microcents: u64,
        per_unit_microcents: u64,
        submission: Duration,
        cancellation: Duration,
    ) -> Result<Self, OrderError> {
        if !(1..=10_000).contains(&participation_bps)
            || submission > Duration::from_secs(86_400)
            || cancellation > Duration::from_secs(86_400)
        {
            return Err(OrderError::ModelUnavailable);
        }
        let bytes = encode(
            version,
            participation_bps,
            fixed_fee_microcents,
            per_unit_microcents,
            (submission, cancellation),
        );
        Ok(Self {
            version,
            participation: participation_bps,
            fixed_fee: fixed_fee_microcents,
            unit_fee: per_unit_microcents,
            submission,
            cancellation,
            digest: ArtifactDigest::of(&bytes),
        })
    }
    /// Model revision and digest; any changed numeric parameter changes identity.
    #[must_use]
    pub const fn identity(self) -> (SemanticReference, ArtifactDigest) {
        (self.version, self.digest)
    }
    /// Versioned complete profile artifact to retain alongside the finite plan.
    #[must_use]
    pub fn canonical_bytes(self) -> Vec<u8> {
        encode(
            self.version,
            self.participation,
            self.fixed_fee,
            self.unit_fee,
            (self.submission, self.cancellation),
        )
    }
    /// Whole-unit participation cap, rounded DOWN; never fabricate fractional units.
    ///
    /// # Errors
    /// Returns `Quantity` if the result cannot fit a whole-unit quantity.
    pub fn capacity(self, coefficient: i128, scale: u32) -> Result<u64, OrderError> {
        let numerator = u128::try_from(coefficient)
            .ok()
            .and_then(|n| n.checked_mul(u128::from(self.participation)))
            .ok_or(OrderError::Quantity)?;
        let denominator = 10_u128
            .checked_pow(scale)
            .and_then(|n| n.checked_mul(10_000))
            .ok_or(OrderError::Quantity)?;
        u64::try_from(numerator / denominator).map_err(|_| OrderError::Quantity)
    }
    /// Cumulative order fee in cents. Split fills cannot multiply fixed fees or
    /// rounding. An explicit P08 total fee cap remains independently enforced.
    ///
    /// # Errors
    /// Returns `Quantity` on checked arithmetic/conversion overflow.
    pub fn fee_cents(self, filled_units: u64) -> Result<u64, OrderError> {
        if filled_units == 0 {
            return Ok(0);
        }
        let micros = u128::from(self.unit_fee)
            .checked_mul(u128::from(filled_units))
            .and_then(|n| n.checked_add(u128::from(self.fixed_fee)))
            .ok_or(OrderError::Quantity)?;
        u64::try_from(micros.div_ceil(1_000_000)).map_err(|_| OrderError::Quantity)
    }
    /// Submission becomes executable at this virtual instant, not receipt time.
    ///
    /// # Errors
    /// Returns `Time` for an unrepresentable timestamp.
    pub fn submission_due(self, submitted: PreciseInstant) -> Result<PreciseInstant, OrderError> {
        add(submitted, self.submission)
    }
    /// Pending cancellation confirms at this virtual instant; until then fills
    /// remain possible. Canonical sequence resolves equal-time races.
    ///
    /// # Errors
    /// Returns `Time` for an unrepresentable timestamp.
    pub fn cancellation_due(self, requested: PreciseInstant) -> Result<PreciseInstant, OrderError> {
        add(requested, self.cancellation)
    }
}
impl std::fmt::Debug for ExecutionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionProfile")
            .field("digest", &self.digest)
            .finish_non_exhaustive()
    }
}
fn encode(
    version: SemanticReference,
    participation: u16,
    fixed: u64,
    unit: u64,
    delays: (Duration, Duration),
) -> Vec<u8> {
    let mut bytes = b"EA-EXECUTION1/TRADE-SIDE/CEIL-CUMULATIVE-CENTS\0".to_vec();
    let (id, revision) = version.version();
    bytes.extend(id.get().to_be_bytes());
    bytes.extend(revision.get().to_be_bytes());
    bytes.extend(participation.to_be_bytes());
    bytes.extend(fixed.to_be_bytes());
    bytes.extend(unit.to_be_bytes());
    bytes.extend(delays.0.as_nanos().to_be_bytes());
    bytes.extend(delays.1.as_nanos().to_be_bytes());
    bytes
}
fn add(at: PreciseInstant, delay: Duration) -> Result<PreciseInstant, OrderError> {
    let (seconds, nanos) = at.unix_parts();
    let total = u128::from(nanos) + u128::from(delay.subsec_nanos());
    let seconds = seconds
        .checked_add(i64::try_from(delay.as_secs()).map_err(|_| OrderError::Time)?)
        .and_then(|s| s.checked_add(i64::try_from(total / 1_000_000_000).ok()?))
        .ok_or(OrderError::Time)?;
    PreciseInstant::from_unix_parts(
        seconds,
        u32::try_from(total % 1_000_000_000).map_err(|_| OrderError::Time)?,
    )
    .map_err(|_| OrderError::Time)
}
