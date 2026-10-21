//! Account payoff rounds once; this pure attribution cannot create or lose cents.
use super::{SettlementCalculation, SettlementError};
use crate::{execution::PositionCost, risk::RiskStrategyId};

/// One deterministic attribution of an already-rounded account/instrument result.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SettlementAttribution {
    strategy: RiskStrategyId,
    units: u64,
    gross: u64,
    fees: u64,
    postings: [i128; 4],
}
impl SettlementAttribution {
    /// Strategy, whole contracts, gross/fee cents and cash/cost/realized/fee postings.
    #[must_use]
    pub const fn parts(self) -> (RiskStrategyId, u64, u64, u64, [i128; 4]) {
        (
            self.strategy,
            self.units,
            self.gross,
            self.fees,
            self.postings,
        )
    }
}
impl std::fmt::Debug for SettlementAttribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettlementAttribution(..)")
    }
}
impl SettlementCalculation {
    /// Attribute one whole-account payoff by cumulative quantity proportion in
    /// ascending strategy-ID order, independent of input/database row order.
    /// Each difference of cumulative floors conserves gross/fees; the final
    /// attribution receives the remaining cents. Release actual strategy costs.
    ///
    /// # Errors
    /// Reject more than 32 entries, duplicate strategies, overflow, or quantity/cost
    /// totals inconsistent with this calculated account lot. Empty/zero lots
    /// yield no attributions only when the calculated lot itself is empty.
    pub fn allocate(
        self,
        positions: &[(RiskStrategyId, PositionCost)],
    ) -> Result<Vec<SettlementAttribution>, SettlementError> {
        if positions.len() > 32 {
            return Err(SettlementError::Position);
        }
        let mut ordered = positions.to_vec();
        ordered.sort_by_key(|p| p.0.get());
        if ordered.windows(2).any(|p| matches!(p,[a,b] if a.0==b.0)) {
            return Err(SettlementError::Position);
        }
        let mut units = 0_u64;
        let mut cost = 0_u64;
        for (_, position) in &ordered {
            units = units
                .checked_add(position.values().0)
                .ok_or(SettlementError::Arithmetic)?;
            cost = cost
                .checked_add(position.values().1)
                .ok_or(SettlementError::Arithmetic)?;
        }
        if units != self.units || -i128::from(cost) != self.postings[1] {
            return Err(SettlementError::Position);
        }
        let mut prefix = 0_u64;
        let mut previous = (0_u64, 0_u64);
        let mut result = Vec::new();
        for (strategy, position) in ordered {
            let (q, c) = position.values();
            if q == 0 {
                continue;
            }
            prefix = prefix.checked_add(q).ok_or(SettlementError::Arithmetic)?;
            // Two u64 factors fit in u128; increasing prefixes keep the
            // cumulative floors monotonic, so differences cannot underflow.
            let gross =
                u64::try_from(u128::from(prefix) * u128::from(self.gross) / u128::from(units))
                    .map_err(|_| SettlementError::Arithmetic)?;
            let fees =
                u64::try_from(u128::from(prefix) * u128::from(self.fees) / u128::from(units))
                    .map_err(|_| SettlementError::Arithmetic)?;
            let (g, f) = (gross - previous.0, fees - previous.1);
            previous = (gross, fees);
            result.push(SettlementAttribution {
                strategy,
                units: q,
                gross: g,
                fees: f,
                postings: [
                    i128::from(g) - i128::from(f),
                    -i128::from(c),
                    i128::from(c) - i128::from(g),
                    i128::from(f),
                ],
            });
        }
        Ok(result)
    }
}
