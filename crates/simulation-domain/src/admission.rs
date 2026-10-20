//! Synthetic-only, exact reservation arithmetic. No funding or authorization token.

mod encoding;

use crate::{
    ArtifactDigest,
    orders::{OrderDefinition, OrderPrice, OrderSide},
    risk::UsdCents,
};
use edgeagent_market_data_domain::{EvidenceInterval, PreciseInstant};

/// Payload-free reservation rejection, never permission to consume funds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReservationError {
    /// Invalid units, tick, sequence, or quote/order association.
    Terms,
    /// Outside quote validity or order lifetime.
    Stale,
    /// Exact upward-rounded debit cannot fit USD cents.
    Overflow,
}
impl std::fmt::Display for ReservationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reservation rejected: {self:?}")
    }
}
impl std::error::Error for ReservationError {}

/// Explicit synthetic execution ceiling and fee cap, not a market/provider quote.
/// The service owns the immutable catalog; requests carry only this fingerprint.
#[derive(Clone, Eq, PartialEq)]
pub struct SyntheticReservationQuote {
    order: OrderDefinition,
    ceiling: OrderPrice,
    fees: UsdCents,
    validity: EvidenceInterval,
    sequence: u64,
    at: PreciseInstant,
    bytes: Vec<u8>,
}
impl SyntheticReservationQuote {
    /// Freeze a service-authored synthetic quote and canonical account input slot.
    /// Zero fees must be explicit; no default, caller amount or unbounded price.
    ///
    /// # Errors
    /// Returns `Terms` for invalid sequence or price units/ticks; `Overflow` for
    /// unrepresentable cash protection. Time validity is assessed at admission.
    pub fn new(
        order: OrderDefinition,
        ceiling: OrderPrice,
        fees: UsdCents,
        validity: EvidenceInterval,
        sequence: u64,
        at: PreciseInstant,
    ) -> Result<Self, ReservationError> {
        if sequence == 0 || sequence > i64::MAX as u64 {
            return Err(ReservationError::Terms);
        }
        order
            .instrument()
            .price(ceiling)
            .map_err(|_| ReservationError::Terms)?;
        let mut quote = Self {
            order,
            ceiling,
            fees,
            validity,
            sequence,
            at,
            bytes: Vec::new(),
        };
        quote.cash()?;
        quote.bytes = encoding::quote(&quote)?;
        Ok(quote)
    }
    /// Immutable complete order terms; the reference does not authorize ownership.
    #[must_use]
    pub const fn order(&self) -> &OrderDefinition {
        &self.order
    }
    /// Canonical source sequence and injected virtual decision instant.
    #[must_use]
    pub const fn position(&self) -> (u64, PreciseInstant) {
        (self.sequence, self.at)
    }
    /// Retained price protection and explicit total-order fee ceiling.
    #[must_use]
    pub const fn protection(&self) -> (OrderPrice, UsdCents) {
        (self.ceiling, self.fees)
    }
    /// Content-derived quote identity; binds all terms, metadata and input position.
    #[must_use]
    pub fn fingerprint(&self) -> ArtifactDigest {
        ArtifactDigest::of(&self.bytes)
    }
    /// Versioned retained bytes. Never use redacted Debug as a persisted artifact.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Require a fresh quote at its frozen canonical input time and within lifetime.
    ///
    /// # Errors
    /// Returns `Stale` for any time mismatch or half-open boundary violation.
    pub fn validate_at(&self, at: PreciseInstant) -> Result<(), ReservationError> {
        let (start, end) = self.validity.bounds();
        let (submitted, deadline) = self.order.lifetime().bounds();
        if at != self.at || at < start || at >= end || at < submitted || at >= deadline {
            return Err(ReservationError::Stale);
        }
        Ok(())
    }
    /// Total cash to reserve. Buys reserve ceil(notional in cents) plus fees;
    /// sells reserve fees only and separately require the full long inventory.
    /// Sale proceeds are never available for admission before a future fill.
    ///
    /// # Errors
    /// Returns `Overflow` on checked product/conversion/addition failure.
    pub fn cash(&self) -> Result<UsdCents, ReservationError> {
        if self.order.terms().0 == OrderSide::SellToClose {
            return Ok(self.fees);
        }
        let (coefficient, scale) = match self.ceiling {
            OrderPrice::PerShare(price) => price.decimal_parts(),
            OrderPrice::Premium(points) => points.decimal_parts(),
        };
        let (multiplier, multiplier_scale) = self
            .order
            .instrument()
            .option_specification()
            .map_or((1, 0), |spec| {
                spec.value().terms().quotation().1.decimal_parts()
            });
        let numerator = u128::try_from(coefficient)
            .ok()
            .and_then(|value| value.checked_mul(u128::try_from(multiplier).ok()?))
            .and_then(|value| value.checked_mul(u128::from(self.order.terms().2.parts().1)))
            .and_then(|value| value.checked_mul(100))
            .ok_or(ReservationError::Overflow)?;
        let denominator = 10_u128
            .checked_pow(scale + multiplier_scale)
            .ok_or(ReservationError::Overflow)?;
        let cents = numerator / denominator + u128::from(numerator % denominator != 0);
        let cents = u64::try_from(cents).map_err(|_| ReservationError::Overflow)?;
        Ok(UsdCents::new(
            cents
                .checked_add(self.fees.get())
                .ok_or(ReservationError::Overflow)?,
        ))
    }
}
impl std::fmt::Debug for SyntheticReservationQuote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntheticReservationQuote")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}
