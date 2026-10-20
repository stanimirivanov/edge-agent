use super::ExecutionProfile;
use crate::orders::{OrderDefinition, OrderError, OrderPrice, OrderSide};

// USD/share and premium multipliers each have <=8 decimals: this fixed unit is
// 10^-14 cents. It preserves their exact product without floating point or a
// rational denominator that changes between fills.
const CENT_SCALE: u32 = 14;
const CENT_DENOMINATOR: u128 = 100_000_000_000_000;

/// Long-only weighted-average book cost. The remaining position retains division
/// remainders, so closing the last unit releases exactly the remaining cost.
/// Fees remain separate expenses; cost is not a market valuation or risk mark.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PositionCost {
    quantity: u64,
    cents: u64,
}
impl PositionCost {
    /// Restore explicit authoritative quantity and total cost; no inferred basis.
    ///
    /// # Errors
    /// Returns `Quantity` for a nonzero cost without a position.
    pub fn new(quantity: u64, cost_cents: u64) -> Result<Self, OrderError> {
        if quantity == 0 && cost_cents != 0 {
            return Err(OrderError::Quantity);
        }
        Ok(Self {
            quantity,
            cents: cost_cents,
        })
    }
    /// Whole units and aggregate remaining book cost in USD cents.
    #[must_use]
    pub const fn values(self) -> (u64, u64) {
        (self.quantity, self.cents)
    }
    pub(super) fn apply(
        self,
        side: OrderSide,
        units: u64,
        gross: u64,
    ) -> Result<(Self, i128), OrderError> {
        match side {
            OrderSide::BuyToOpen => {
                let quantity = self
                    .quantity
                    .checked_add(units)
                    .ok_or(OrderError::Quantity)?;
                let cents = self.cents.checked_add(gross).ok_or(OrderError::Quantity)?;
                Ok((Self { quantity, cents }, i128::from(gross)))
            }
            OrderSide::SellToClose => {
                if units > self.quantity || self.quantity == 0 {
                    return Err(OrderError::Quantity);
                }
                let cost = u64::try_from(
                    u128::from(self.cents) * u128::from(units) / u128::from(self.quantity),
                )
                .map_err(|_| OrderError::Quantity)?;
                Ok((
                    Self {
                        quantity: self.quantity - units,
                        cents: self.cents - cost,
                    },
                    -i128::from(cost),
                ))
            }
        }
    }
}
impl std::fmt::Debug for PositionCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PositionCost(..)")
    }
}

/// One checked incremental balanced posting and its cumulative order economics.
/// Persist the position, totals and postings in the same account transaction.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FillAccounting {
    exact: u128,
    gross: u64,
    fees: u64,
    units: u64,
    position: PositionCost,
    postings: [i128; 4],
}
impl FillAccounting {
    /// Recompute cumulative totals from the checked retained lifecycle journal.
    /// Stored counters are caches, never independent financial authority.
    ///
    /// # Errors
    /// Returns `Quantity` for arithmetic overflow or `Unit` for incompatible prices.
    pub fn retained_totals(
        order: &crate::orders::Order,
        profile: ExecutionProfile,
    ) -> Result<(u128, u64, u64, u64), OrderError> {
        let mut exact = 0_u128;
        let mut units = 0_u64;
        for event in order.events() {
            if let crate::orders::OrderEventKind::Filled(fill) = event.effect().1 {
                let (quantity, price) = fill.evidence().0.values();
                exact = exact
                    .checked_add(notional(order.definition(), price, quantity.parts().1)?)
                    .ok_or(OrderError::Quantity)?;
                units = units
                    .checked_add(quantity.parts().1)
                    .ok_or(OrderError::Quantity)?;
            }
        }
        Ok((
            exact,
            units,
            rounded(exact, order.definition().terms().0)?,
            profile.fee_cents(units)?,
        ))
    }
    /// Compute a fill against retained cumulative totals. Buy notional rounds up,
    /// sell notional down at ORDER level; fees round up once cumulatively. The
    /// four debits are cash, position cost, realized P/L expense and fee expense.
    ///
    /// # Errors
    /// Returns `Quantity`, `Unit` or `Price` for overflow/inconsistent totals,
    /// insufficient long inventory, or an independently exceeded fee ceiling.
    pub fn calculate(
        order: &OrderDefinition,
        profile: ExecutionProfile,
        price: OrderPrice,
        units: u64,
        previous: (u128, u64),
        fee_cap_cents: u64,
        position: PositionCost,
    ) -> Result<Self, OrderError> {
        if units == 0 {
            return Err(OrderError::Quantity);
        }
        order.instrument().price(price)?;
        let side = order.terms().0;
        let old_gross = rounded(previous.0, side)?;
        let old_fee = profile.fee_cents(previous.1)?;
        let exact = previous
            .0
            .checked_add(notional(order, price, units)?)
            .ok_or(OrderError::Quantity)?;
        let total_units = previous.1.checked_add(units).ok_or(OrderError::Quantity)?;
        if total_units > order.terms().2.parts().1 {
            return Err(OrderError::Quantity);
        }
        let total_gross = rounded(exact, side)?;
        let total_fee = profile.fee_cents(total_units)?;
        if total_fee > fee_cap_cents {
            return Err(OrderError::Price);
        }
        let gross = total_gross
            .checked_sub(old_gross)
            .ok_or(OrderError::Quantity)?;
        let fee = total_fee.checked_sub(old_fee).ok_or(OrderError::Quantity)?;
        let (position, cost) = position.apply(side, units, gross)?;
        let cash = match side {
            OrderSide::BuyToOpen => -i128::from(gross) - i128::from(fee),
            OrderSide::SellToClose => i128::from(gross) - i128::from(fee),
        };
        let realized = -cash - cost - i128::from(fee);
        Ok(Self {
            exact,
            gross: total_gross,
            fees: total_fee,
            units: total_units,
            position,
            postings: [cash, cost, realized, i128::from(fee)],
        })
    }
    /// Exact 10^-14 cent total, whole-unit total, rounded gross and fee total.
    #[must_use]
    pub const fn totals(self) -> (u128, u64, u64, u64) {
        (self.exact, self.units, self.gross, self.fees)
    }
    /// Updated weighted-average long position and balanced signed debits.
    #[must_use]
    pub const fn effects(self) -> (PositionCost, [i128; 4]) {
        (self.position, self.postings)
    }
}
impl std::fmt::Debug for FillAccounting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FillAccounting(..)")
    }
}
fn rounded(exact: u128, side: OrderSide) -> Result<u64, OrderError> {
    let value = match side {
        OrderSide::BuyToOpen => exact.div_ceil(CENT_DENOMINATOR),
        OrderSide::SellToClose => exact / CENT_DENOMINATOR,
    };
    u64::try_from(value).map_err(|_| OrderError::Quantity)
}
fn notional(order: &OrderDefinition, price: OrderPrice, units: u64) -> Result<u128, OrderError> {
    let (coefficient, scale) = match price {
        OrderPrice::PerShare(p) => p.decimal_parts(),
        OrderPrice::Premium(p) => p.decimal_parts(),
    };
    let (multiplier, ms) = order
        .instrument()
        .option_specification()
        .map_or((1, 0), |spec| {
            spec.value().terms().quotation().1.decimal_parts()
        });
    let scale = scale.checked_add(ms).ok_or(OrderError::Quantity)?;
    // Convert the USD product to the fixed cent unit without discarding digits.
    let exponent = (CENT_SCALE + 2)
        .checked_sub(scale)
        .ok_or(OrderError::Quantity)?;
    u128::try_from(coefficient)
        .ok()
        .and_then(|n| n.checked_mul(u128::try_from(multiplier).ok()?))
        .and_then(|n| n.checked_mul(u128::from(units)))
        .and_then(|n| n.checked_mul(10_u128.checked_pow(exponent)?))
        .ok_or(OrderError::Quantity)
}
