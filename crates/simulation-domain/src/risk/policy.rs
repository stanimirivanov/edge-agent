use super::{
    RiskAccountId, RiskError, RiskIncrease, RiskLimits, RiskOperatorId, RiskReason, RiskSession,
    RiskStrategyId, RiskUsage, UsdCents,
};
use crate::ArtifactDigest;
use edgeagent_market_data_domain::{PreciseInstant, SemanticReference};
use std::time::Duration;

/// One strategy's complete current usage and actual valuation availability time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrategyValuation {
    pub(super) strategy: RiskStrategyId,
    pub(super) usage: RiskUsage,
    pub(super) valued: PreciseInstant,
}
impl StrategyValuation {
    /// Include positions, reservations and open orders even for inactive strategies.
    /// Zero usage still needs an explicit fresh row; omitted is not equivalent to zero.
    #[must_use]
    pub const fn new(strategy: RiskStrategyId, usage: RiskUsage, valued: PreciseInstant) -> Self {
        Self {
            strategy,
            usage,
            valued,
        }
    }
}

/// Bounded authoritative account-view claim, NOT a query projection or proof of a lock.
/// P08 must assemble every row in the same account transaction and bind valuation
/// artifacts/rounding/baselines to the frozen run configuration.
#[derive(Clone, Eq, PartialEq)]
pub struct RiskSnapshot {
    pub(super) account: RiskAccountId,
    pub(super) session: RiskSession,
    pub(super) rows: Vec<StrategyValuation>,
}
impl RiskSnapshot {
    /// Preserve 1..=32 distinct strategy valuations, sorted by stable strategy ID.
    ///
    /// # Errors
    /// Returns `Configuration` before cloning on invalid bounds, or on duplicate IDs.
    pub fn new(
        account: RiskAccountId,
        session: RiskSession,
        rows: &[StrategyValuation],
    ) -> Result<Self, RiskError> {
        if rows.is_empty() || rows.len() > 32 {
            return Err(RiskError::Configuration);
        }
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| row.strategy);
        if rows
            .windows(2)
            .any(|pair| pair.first().map(|r| r.strategy) == pair.last().map(|r| r.strategy))
        {
            return Err(RiskError::Configuration);
        }
        Ok(Self {
            account,
            session,
            rows,
        })
    }
}
impl std::fmt::Debug for RiskSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskSnapshot")
            .field("strategies", &self.rows.len())
            .finish_non_exhaustive()
    }
}

/// Immutable risk policy: inclusive account caps AND every strategy's caps.
/// Revision is service-assigned; fingerprint independently binds all parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskPolicy {
    pub(super) revision: SemanticReference,
    pub(super) max_age: Duration,
    account: RiskLimits,
    strategies: Vec<(RiskStrategyId, RiskLimits)>,
    pub(super) operators: Vec<RiskOperatorId>,
    fingerprint: ArtifactDigest,
}
impl RiskPolicy {
    /// Pin nonzero freshness, 1..=32 distinct strategies, and 1..=16 distinct
    /// reset operators. Authentication must map the actor before domain calls.
    ///
    /// # Errors
    /// Returns `Configuration` for absent/duplicate/oversized inputs or zero age.
    pub fn new(
        revision: SemanticReference,
        max_age: Duration,
        account: RiskLimits,
        strategies: &[(RiskStrategyId, RiskLimits)],
        operators: &[RiskOperatorId],
    ) -> Result<Self, RiskError> {
        if max_age.is_zero()
            || strategies.is_empty()
            || strategies.len() > 32
            || operators.is_empty()
            || operators.len() > 16
        {
            return Err(RiskError::Configuration);
        }
        let mut strategies = strategies.to_vec();
        strategies.sort_by_key(|row| row.0);
        let mut operators = operators.to_vec();
        operators.sort_unstable();
        if strategies
            .windows(2)
            .any(|pair| pair.first().map(|r| r.0) == pair.last().map(|r| r.0))
            || operators.windows(2).any(|pair| pair.first() == pair.last())
        {
            return Err(RiskError::Configuration);
        }
        // Versioned explicit big-endian encoding; Rust Hash/Debug are not artifacts.
        let mut bytes = b"EA-RISK1/SUM-ADVERSE/USD-CENTS\0".to_vec();
        let (id, rev) = revision.version();
        bytes.extend(id.get().to_be_bytes());
        bytes.extend(rev.get().to_be_bytes());
        bytes.extend(max_age.as_nanos().to_be_bytes());
        encode_usage(account.0, &mut bytes);
        bytes.extend((strategies.len() as u64).to_be_bytes());
        for (strategy, limits) in &strategies {
            bytes.extend(strategy.get().to_be_bytes());
            encode_usage(limits.0, &mut bytes);
        }
        bytes.extend((operators.len() as u64).to_be_bytes());
        for operator in &operators {
            bytes.extend(operator.get().to_be_bytes());
        }
        let fingerprint = ArtifactDigest::of(&bytes);
        Ok(Self {
            revision,
            max_age,
            account,
            strategies,
            operators,
            fingerprint,
        })
    }
    /// Bind this digest into P05's configuration artifact; revision alone is not content.
    #[must_use]
    pub const fn fingerprint(&self) -> ArtifactDigest {
        self.fingerprint
    }
    pub(super) fn includes_strategy(&self, strategy: RiskStrategyId) -> bool {
        self.strategies.iter().any(|row| row.0 == strategy)
    }
    pub(super) fn current(
        &self,
        snapshot: &RiskSnapshot,
        at: PreciseInstant,
    ) -> Result<Option<RiskReason>, RiskError> {
        if snapshot.rows.len() != self.strategies.len()
            || snapshot
                .rows
                .iter()
                .zip(&self.strategies)
                .any(|(row, rule)| row.strategy != rule.0)
        {
            return Err(RiskError::Scope);
        }
        // Validate future times even if the account is closed/stale/latched.
        if snapshot.rows.iter().any(|row| row.valued > at) {
            return Err(RiskError::Clock);
        }
        if !snapshot.session.contains(at) {
            return Ok(Some(RiskReason::SessionClosed));
        }
        if snapshot
            .rows
            .iter()
            .any(|row| elapsed(at, row.valued) > self.max_age.as_nanos())
        {
            return Ok(Some(RiskReason::StaleValuation));
        }
        self.limits(snapshot, None)
    }
    pub(super) fn projected(
        &self,
        snapshot: &RiskSnapshot,
        increase: RiskIncrease,
    ) -> Result<Option<RiskReason>, RiskError> {
        if !self.strategies.iter().any(|row| row.0 == increase.strategy) {
            return Err(RiskError::Scope);
        }
        self.limits(snapshot, Some(increase))
    }
    fn limits(
        &self,
        snapshot: &RiskSnapshot,
        increase: Option<RiskIncrease>,
    ) -> Result<Option<RiskReason>, RiskError> {
        let mut total = RiskUsage::total(UsdCents::new(0), UsdCents::new(0), 0);
        let mut first_strategy = None;
        for (row, (_, limits)) in snapshot.rows.iter().zip(&self.strategies) {
            let usage =
                if let Some(change) = increase.filter(|change| change.strategy == row.strategy) {
                    row.usage.plus(change.usage)?
                } else {
                    row.usage
                };
            total = total.plus(usage)?;
            first_strategy = first_strategy.or_else(|| exceeds(usage, *limits, Some(row.strategy)));
        }
        Ok(exceeds(total, self.account, None).or(first_strategy))
    }
}
fn encode_usage(usage: RiskUsage, bytes: &mut Vec<u8>) {
    bytes.extend(usage.gross.get().to_be_bytes());
    bytes.extend(usage.loss.get().to_be_bytes());
    bytes.extend(usage.orders.to_be_bytes());
}
fn elapsed(later: PreciseInstant, earlier: PreciseInstant) -> u128 {
    let nanos = |at: PreciseInstant| {
        let (s, n) = at.unix_parts();
        i128::from(s) * 1_000_000_000 + i128::from(n)
    };
    (nanos(later) - nanos(earlier)).unsigned_abs()
}
fn exceeds(
    usage: RiskUsage,
    limit: RiskLimits,
    strategy: Option<RiskStrategyId>,
) -> Option<RiskReason> {
    if usage.gross > limit.0.gross {
        Some(strategy.map_or(RiskReason::AccountGross, RiskReason::StrategyGross))
    } else if usage.loss > limit.0.loss {
        Some(strategy.map_or(RiskReason::AccountLoss, RiskReason::StrategyLoss))
    } else if usage.orders > limit.0.orders {
        Some(strategy.map_or(RiskReason::AccountOrders, RiskReason::StrategyOrders))
    } else {
        None
    }
}
