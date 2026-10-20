use super::{
    DryRunMode, OrderError, OrderId, OrderKind, OrderPrice, OrderQuantity, OrderSide, QuantityUnit,
};
use crate::{
    ArtifactDigest,
    risk::{RiskAccountId, RiskStrategyId},
};
use edgeagent_market_data_domain::{
    EvidenceInterval, InstrumentId, OptionSpecification, PreciseInstant, ReferenceId,
    ReferenceRevision, ReferenceVersion,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum InstrumentKind {
    Equity(InstrumentId),
    IndexOption(Box<ReferenceVersion<OptionSpecification>>),
}
/// Structurally supported product with immutable option source/specification metadata.
/// Construction grants no entitlement, calendar, pricing or funded eligibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderInstrument(InstrumentKind);
impl OrderInstrument {
    /// Bind a qualified equity/ETF identity; the caller verifies asset classification.
    #[must_use]
    pub const fn equity(id: InstrumentId) -> Self {
        Self(InstrumentKind::Equity(id))
    }
    /// Retain an exact cash-settled European index-style specification version.
    ///
    /// # Errors
    /// Returns `Specification` for every unsupported exercise/settlement/underlying shape.
    pub fn index_option(
        specification: ReferenceVersion<OptionSpecification>,
    ) -> Result<Self, OrderError> {
        specification
            .value()
            .validate_initial_simulation_shape()
            .map_err(|_| OrderError::Specification)?;
        Ok(Self(InstrumentKind::IndexOption(Box::new(specification))))
    }
    /// Stable product identity, never a provider symbol or index proxy.
    #[must_use]
    pub fn id(&self) -> InstrumentId {
        match &self.0 {
            InstrumentKind::Equity(id) => *id,
            InstrumentKind::IndexOption(spec) => spec.value().id(),
        }
    }
    /// Exact option reference identity and revision, when applicable.
    #[must_use]
    pub fn specification(&self) -> Option<(ReferenceId, ReferenceRevision)> {
        match &self.0 {
            InstrumentKind::Equity(_) => None,
            InstrumentKind::IndexOption(spec) => Some((spec.id(), spec.revision())),
        }
    }
    pub(super) fn price(&self, price: OrderPrice) -> Result<(), OrderError> {
        match (&self.0, price) {
            (InstrumentKind::Equity(_), OrderPrice::PerShare(_)) => Ok(()),
            (InstrumentKind::IndexOption(spec), OrderPrice::Premium(points)) => {
                if points.is_multiple_of(spec.value().terms().quotation().0) {
                    Ok(())
                } else {
                    Err(OrderError::Price)
                }
            }
            _ => Err(OrderError::Unit),
        }
    }
    fn validate(
        &self,
        quantity: OrderQuantity,
        lifetime: EvidenceInterval,
    ) -> Result<(), OrderError> {
        let (submitted, expires) = lifetime.bounds();
        match &self.0 {
            InstrumentKind::Equity(_) if quantity.parts().0 == QuantityUnit::Shares => Ok(()),
            InstrumentKind::IndexOption(spec) if quantity.parts().0 == QuantityUnit::Contracts => {
                let (effective_start, effective_end) = spec.value().effective().bounds();
                let (activation, last_trading) = spec.value().schedule().trading_bounds();
                if PreciseInstant::from_millis(spec.known_at()) > submitted
                    || PreciseInstant::from_millis(effective_start) > submitted
                    || PreciseInstant::from_millis(activation) > submitted
                    || expires > PreciseInstant::from_millis(effective_end)
                    || expires > PreciseInstant::from_millis(last_trading)
                {
                    Err(OrderError::Specification)
                } else {
                    Ok(())
                }
            }
            _ => Err(OrderError::Unit),
        }
    }
}

/// Immutable run/account/strategy/order ownership. Identity does not authorize access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderScope {
    pub(super) run: ArtifactDigest,
    pub(super) account: RiskAccountId,
    pub(super) strategy: RiskStrategyId,
    pub(super) order: OrderId,
}
impl OrderScope {
    /// Bind P05 run identity and P06 allocation identities to a stable order ID.
    #[must_use]
    pub const fn new(
        run: ArtifactDigest,
        account: RiskAccountId,
        strategy: RiskStrategyId,
        order: OrderId,
    ) -> Self {
        Self {
            run,
            account,
            strategy,
            order,
        }
    }
    /// Run, account, strategy and order identity, in that order.
    #[must_use]
    pub const fn identity(self) -> (ArtifactDigest, RiskAccountId, RiskStrategyId, OrderId) {
        (self.run, self.account, self.strategy, self.order)
    }
}

/// Domain submission contract, not an admitted order or published wire command.
/// Explicit half-open lifetime is the only initial time-in-force: no inferred day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderDefinition {
    mode: DryRunMode,
    pub(super) scope: OrderScope,
    pub(super) instrument: OrderInstrument,
    pub(super) side: OrderSide,
    pub(super) kind: OrderKind,
    pub(super) quantity: OrderQuantity,
    pub(super) lifetime: EvidenceInterval,
}
impl OrderDefinition {
    /// Validate mode, quantity/price units, ticks and the entire pinned option
    /// lifetime without rounding nanoseconds into a millisecond specification.
    ///
    /// # Errors
    /// Returns `Unit`, `Price` or `Specification` for inconsistent product terms.
    pub fn new(
        mode: DryRunMode,
        scope: OrderScope,
        instrument: OrderInstrument,
        side: OrderSide,
        kind: OrderKind,
        quantity: OrderQuantity,
        lifetime: EvidenceInterval,
    ) -> Result<Self, OrderError> {
        instrument.validate(quantity, lifetime)?;
        if let OrderKind::Limit(price) = kind {
            instrument.price(price)?;
        }
        Ok(Self {
            mode,
            scope,
            instrument,
            side,
            kind,
            quantity,
            lifetime,
        })
    }
    /// Immutable ownership.
    #[must_use]
    pub const fn scope(&self) -> OrderScope {
        self.scope
    }
    /// Pinned instrument/specification metadata.
    #[must_use]
    pub const fn instrument(&self) -> &OrderInstrument {
        &self.instrument
    }
    /// Side, order style and original whole quantity.
    #[must_use]
    pub const fn terms(&self) -> (OrderSide, OrderKind, OrderQuantity) {
        (self.side, self.kind, self.quantity)
    }
    /// Inclusive submission time and exclusive execution deadline.
    #[must_use]
    pub const fn lifetime(&self) -> EvidenceInterval {
        self.lifetime
    }
    pub(super) fn fill_price(&self, price: OrderPrice) -> Result<(), OrderError> {
        self.instrument.price(price)?;
        if let OrderKind::Limit(limit) = self.kind {
            let worse = match (price, limit) {
                (OrderPrice::PerShare(actual), OrderPrice::PerShare(limit)) => match self.side {
                    OrderSide::BuyToOpen => actual > limit,
                    OrderSide::SellToClose => actual < limit,
                },
                (OrderPrice::Premium(actual), OrderPrice::Premium(limit)) => match self.side {
                    OrderSide::BuyToOpen => actual > limit,
                    OrderSide::SellToClose => actual < limit,
                },
                _ => return Err(OrderError::Unit),
            };
            if worse {
                return Err(OrderError::Price);
            }
        }
        Ok(())
    }
}
