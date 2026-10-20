use super::ExecutionProfile;
use crate::{ArtifactDigest, FrozenInput, Run, orders::*};
use edgeagent_market_data_domain::{
    ContractQuantity, InstrumentId, ObservationValue, PreciseInstant, ReadyEvidence, ReferenceId,
    ReferenceRevision, SemanticReference,
};

/// One qualified, consumed observation's conservative capacity. Callers serialize
/// capacity consumption by `(run, key)` across accounts/strategies/replicas.
/// Repeated option snapshots with the same side timestamp share the SAME budget.
/// Corrections never replenish capacity: this initial model rejects revisions >1.
#[derive(Clone)]
pub struct ExecutableLiquidity {
    key: ArtifactDigest,
    content: ArtifactDigest,
    capacity: u64,
    price: OrderPrice,
    profile: ExecutionProfile,
    run: ArtifactDigest,
    instrument: InstrumentId,
    specification: Option<(ReferenceId, ReferenceRevision)>,
    side: OrderSide,
    input: FrozenInput,
}
impl std::fmt::Debug for ExecutableLiquidity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExecutableLiquidity(..)")
    }
}
impl ExecutableLiquidity {
    /// Select exactly one matching ready observation, also present in P05's
    /// consumed prefix. Bars, index levels, ambiguous matches and corrections
    /// fail closed. Unknown/zero option sizes and zero premium provide no fill.
    /// Artifact bytes/rights/calendar qualification remain the composition's duty.
    ///
    /// # Errors
    /// Returns `Evidence` for mismatched/unsupported provenance and `Quantity`
    /// for unrepresentable capacity. No liquidity is consumed by this function.
    pub fn from_ready(
        run: &Run<'_>,
        order: &OrderDefinition,
        profile: ExecutionProfile,
        ready: &ReadyEvidence<'_>,
        artifacts: &[ArtifactDigest],
    ) -> Result<Self, OrderError> {
        if run.now() != ready.assessment().0 || run.manifest_digest() != order.scope().identity().0
        {
            return Err(OrderError::Evidence);
        }
        let frozen = FrozenInput::from_ready(ready, artifacts).map_err(|_| OrderError::Evidence)?;
        let mut matches = ready
            .observations()
            .iter()
            .zip(&frozen)
            .filter(|(o, _)| o.context().origin().0 == order.instrument().id());
        let (observation, input) = matches.next().ok_or(OrderError::Evidence)?;
        if matches.next().is_some()
            || !observation.context().session_conditions().1.is_empty()
            || input.identity().1 != 1
            || !run
                .consumed_inputs()
                .iter()
                .any(|old| old.supports_reassessment(input))
        {
            return Err(OrderError::Evidence);
        }
        let mut key = b"EA-LIQUIDITY1\0".to_vec();
        key.extend(run.manifest_digest().bytes());
        key.extend(input.feed.to_be_bytes());
        key.extend(order.instrument().id().get().to_be_bytes());
        let mut source_size = Vec::new();
        let (price, capacity) = match (order.instrument().specification(), observation.value()) {
            (None, Some(ObservationValue::Trade(trade))) => {
                key.push(0); // All strategies and both order sides share this trade.
                key.extend(input.identity().0.to_be_bytes());
                let (price, size) = trade.values();
                let (n, s) = size.decimal_parts();
                source_size.push(1);
                source_size.extend(n.to_be_bytes());
                source_size.extend(s.to_be_bytes());
                (OrderPrice::PerShare(price), profile.capacity(n, s)?)
            }
            (Some(spec), Some(ObservationValue::OptionQuote(quote)))
                if quote.specification() == spec =>
            {
                let side = match order.terms().0 {
                    OrderSide::BuyToOpen => {
                        key.push(1);
                        quote.sides().1
                    }
                    OrderSide::SellToClose => {
                        key.push(2);
                        quote.sides().0
                    }
                };
                let (s, n) = side.at().unix_parts();
                key.extend(s.to_be_bytes());
                key.extend(n.to_be_bytes());
                let (price, size) = side.values();
                source_size.push(u8::from(size.is_some()));
                source_size.extend(size.map_or(0, |s| s.get()).to_be_bytes());
                let capacity = if price.decimal_parts().0 == 0 {
                    0
                } else {
                    size.map_or(Ok(0), |size| profile.capacity(i128::from(size.get()), 0))?
                };
                (OrderPrice::Premium(price), capacity)
            }
            _ => return Err(OrderError::Evidence),
        };
        // Side content, not wrapper observation ID: duplicate snapshot wrappers
        // cannot create new capacity; changed contents at that time are conflicts.
        let mut content = key.clone();
        let (n, s) = match price {
            OrderPrice::PerShare(p) => p.decimal_parts(),
            OrderPrice::Premium(p) => p.decimal_parts(),
        };
        content.extend(n.to_be_bytes());
        content.extend(s.to_be_bytes());
        content.extend(capacity.to_be_bytes());
        content.extend(source_size);
        if let Some((id, revision)) = order.instrument().specification() {
            content.push(1);
            content.extend(id.get().to_be_bytes());
            content.extend(revision.get().to_be_bytes());
        } else {
            content.push(0);
        }
        content.extend(profile.identity().1.bytes());
        Ok(Self {
            key: ArtifactDigest::of(&key),
            content: ArtifactDigest::of(&content),
            capacity,
            price,
            profile,
            run: run.manifest_digest(),
            instrument: order.instrument().id(),
            specification: order.instrument().specification(),
            side: order.terms().0,
            input: input.clone(),
        })
    }
    /// Shared budget key, exact execution content and whole-unit capacity.
    #[must_use]
    pub const fn budget(&self) -> (ArtifactDigest, ArtifactDigest, u64) {
        (self.key, self.content, self.capacity)
    }
    /// Propose within remaining displayed participation and order/ceiling limits.
    /// `consumed` is authoritative cumulative usage, not a per-worker counter.
    ///
    /// # Errors
    /// Returns `Quantity` for corrupted consumption or invalid units.
    pub fn propose(
        &self,
        order: &OrderDefinition,
        remaining: OrderQuantity,
        consumed: u64,
        ceiling: OrderPrice,
    ) -> Result<Option<ProposedFill>, OrderError> {
        if order.scope().identity().0 != self.run
            || order.instrument().id() != self.instrument
            || order.instrument().specification() != self.specification
            || (self.specification.is_some() && order.terms().0 != self.side)
            || remaining.parts().0 != order.terms().2.parts().0
        {
            return Err(OrderError::Evidence);
        }
        let available = self
            .capacity
            .checked_sub(consumed)
            .ok_or(OrderError::Quantity)?;
        let count = available.min(remaining.parts().1);
        if count == 0 || order.fill_price(self.price).is_err() {
            return Ok(None);
        }
        // P08 protects buy cash even when the order itself is a market order.
        if order.terms().0 == OrderSide::BuyToOpen {
            let worse = match (self.price, ceiling) {
                (OrderPrice::PerShare(p), OrderPrice::PerShare(c)) => p > c,
                (OrderPrice::Premium(p), OrderPrice::Premium(c)) => p > c,
                _ => return Err(OrderError::Unit),
            };
            if worse {
                return Ok(None);
            }
        }
        let quantity = match remaining.parts().0 {
            QuantityUnit::Shares => OrderQuantity::shares(count)?,
            QuantityUnit::Contracts => OrderQuantity::contracts(
                ContractQuantity::new(count).map_err(|_| OrderError::Quantity)?,
            ),
        };
        Ok(Some(ProposedFill::new(quantity, self.price)))
    }
}

/// P07 execution-port implementation. It proposes against one authoritative
/// remaining budget but never mutates that budget; the account transaction owns
/// consumption. Submission latency uses the retained admission instant.
#[derive(Clone)]
pub struct LiquidityExecution {
    market: ExecutableLiquidity,
    consumed: u64,
    ceiling: OrderPrice,
    admitted: PreciseInstant,
}
impl LiquidityExecution {
    /// Bind already qualified evidence to persisted consumption and protection.
    /// Invalid consumption and incompatible ceiling units fail before proposing.
    ///
    /// # Errors
    /// Returns `Quantity` for exhausted/corrupt consumption, `Unit` for a foreign
    /// price unit or `Time` for an unrepresentable submission due instant.
    pub fn new(
        market: ExecutableLiquidity,
        consumed: u64,
        ceiling: OrderPrice,
        admitted: PreciseInstant,
    ) -> Result<Self, OrderError> {
        if consumed > market.capacity {
            return Err(OrderError::Quantity);
        }
        if !matches!(
            (market.price, ceiling),
            (OrderPrice::PerShare(_), OrderPrice::PerShare(_))
                | (OrderPrice::Premium(_), OrderPrice::Premium(_))
        ) {
            return Err(OrderError::Unit);
        }
        market.profile.submission_due(admitted)?;
        Ok(Self {
            market,
            consumed,
            ceiling,
            admitted,
        })
    }
}
impl std::fmt::Debug for LiquidityExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LiquidityExecution(..)")
    }
}
impl SimulationExecution for LiquidityExecution {
    fn identity(&self) -> (SemanticReference, ArtifactDigest) {
        self.market.profile.identity()
    }
    fn propose(
        &mut self,
        request: SimulationRequest<'_, '_>,
    ) -> Result<Option<ProposedFill>, OrderError> {
        if !request
            .inputs()
            .iter()
            .any(|input| self.market.input.supports_reassessment(input))
        {
            return Err(OrderError::Evidence);
        }
        if request.at() < self.market.profile.submission_due(self.admitted)? {
            return Ok(None);
        }
        self.market.propose(
            request.order(),
            request.remaining(),
            self.consumed,
            self.ceiling,
        )
    }
}
