use edgeagent_market_data_domain::{
    EvidenceInterval, PreciseInstant, SemanticReference, SessionDate,
};

/// Payload-free risk validation and transition errors. Every error denies admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RiskError {
    /// Invalid identity, empty/oversized policy or duplicate strategy/operator.
    Configuration,
    /// Account, session or complete strategy set does not match the policy.
    Scope,
    /// Future valuation or regressing decision time.
    Clock,
    /// Exact aggregate arithmetic or revision cannot advance without overflow.
    Overflow,
    /// The expected risk revision differs from current state.
    Revision,
    /// Actor is not an authorized reset operator in this pinned policy.
    Unauthorized,
    /// Reset requires a latched breach and a fresh, currently healthy snapshot.
    Reset,
    /// Session rollover is not forward, non-overlapping and currently active.
    Session,
}
impl std::fmt::Display for RiskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "simulation risk rejected: {self:?}")
    }
}
impl std::error::Error for RiskError {}

macro_rules! identity {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(std::num::NonZeroU128);
        impl $name {
            /// Construct a nonzero service-assigned identity.
            ///
            /// # Errors
            /// Returns `Configuration` for zero. Identity is not authentication.
            pub fn new(value: u128) -> Result<Self, RiskError> {
                std::num::NonZeroU128::new(value)
                    .map(Self)
                    .ok_or(RiskError::Configuration)
            }
            /// Exact identity, scoped by its owning application.
            #[must_use]
            pub const fn get(self) -> u128 {
                self.0.get()
            }
        }
    };
}
identity!(
    RiskAccountId,
    "Simulated account identity; never a live brokerage account."
);
identity!(
    RiskStrategyId,
    "Strategy allocation identity within a simulated account."
);
identity!(
    RiskOperatorId,
    "Authenticated-principal mapping supplied by the application, not a grant."
);

/// Nonnegative exact USD cents for risk caps/contributions, not price or ledger money.
/// Callers must conservatively convert finer units under a pinned rounding policy;
/// this type never rounds, converts currencies or derives valuations.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct UsdCents(u64);
impl UsdCents {
    /// Preserve an exact nonnegative count; zero is valid.
    #[must_use]
    pub const fn new(cents: u64) -> Self {
        Self(cents)
    }
    /// Exact cents, not USD dollars or quotation points.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl std::fmt::Debug for UsdCents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UsdCents(..)")
    }
}

/// Current strategy usage, including positions AND outstanding reservations.
/// Loss is a nonnegative adverse contribution since the pinned session baseline;
/// gains cannot offset another strategy's loss. No Greeks/margin model is implied.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RiskUsage {
    pub(super) gross: UsdCents,
    pub(super) loss: UsdCents,
    pub(super) orders: u64,
}
impl RiskUsage {
    /// Add position exposure AND outstanding reservations without netting.
    /// Pending cancellations remain reserved until terminal effect is committed.
    ///
    /// # Errors
    /// Returns `Overflow` when combined gross exposure cannot fit exact USD cents.
    pub fn new(
        position: UsdCents,
        reserved: UsdCents,
        loss: UsdCents,
        orders: u64,
    ) -> Result<Self, RiskError> {
        let gross = UsdCents(
            position
                .0
                .checked_add(reserved.0)
                .ok_or(RiskError::Overflow)?,
        );
        Ok(Self::total(gross, loss, orders))
    }
    pub(super) const fn total(gross: UsdCents, loss: UsdCents, orders: u64) -> Self {
        Self {
            gross,
            loss,
            orders,
        }
    }
    pub(super) fn plus(self, other: Self) -> Result<Self, RiskError> {
        Ok(Self::total(
            UsdCents(
                self.gross
                    .0
                    .checked_add(other.gross.0)
                    .ok_or(RiskError::Overflow)?,
            ),
            UsdCents(
                self.loss
                    .0
                    .checked_add(other.loss.0)
                    .ok_or(RiskError::Overflow)?,
            ),
            self.orders
                .checked_add(other.orders)
                .ok_or(RiskError::Overflow)?,
        ))
    }
}
impl std::fmt::Debug for RiskUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RiskUsage(..)")
    }
}

/// Inclusive gross/session-loss/open-order ceilings; zero disables positive use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiskLimits(pub(super) RiskUsage);
impl RiskLimits {
    /// Set explicit limits, without defaults or implicit account overrides.
    #[must_use]
    pub const fn new(gross: UsdCents, loss: UsdCents, orders: u64) -> Self {
        Self(RiskUsage::total(gross, loss, orders))
    }
}

/// Prospective nonnegative increase, not an order, reservation or admission token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiskIncrease {
    pub(super) strategy: RiskStrategyId,
    pub(super) usage: RiskUsage,
}
impl RiskIncrease {
    /// Add gross exposure and open orders to exactly one strategy. Existing losses
    /// cannot be reduced/netted away through a proposed increase.
    #[must_use]
    pub const fn new(strategy: RiskStrategyId, gross: UsdCents, orders: u64) -> Self {
        Self {
            strategy,
            usage: RiskUsage::total(gross, UsdCents(0), orders),
        }
    }
}

/// Explicit half-open calendar-bound risk session. Not an inferred exchange day.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiskSession {
    pub(super) date: SessionDate,
    pub(super) interval: EvidenceInterval,
    pub(super) calendar: SemanticReference,
}
impl RiskSession {
    /// Bind a checked local session date, exact interval and qualified calendar
    /// revision. Caller verifies that these claims agree with source reference data.
    #[must_use]
    pub const fn new(
        date: SessionDate,
        interval: EvidenceInterval,
        calendar: SemanticReference,
    ) -> Self {
        Self {
            date,
            interval,
            calendar,
        }
    }
    pub(super) fn contains(self, at: PreciseInstant) -> bool {
        let (start, end) = self.interval.bounds();
        start <= at && at < end
    }
}

/// Stable first-denial categories; ordering is documented in the risk guide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RiskReason {
    /// Decision time falls outside the current explicit session.
    SessionClosed,
    /// At least one configured strategy valuation is too old.
    StaleValuation,
    /// Sum of all strategies' gross exposure/reservations exceeds the account cap.
    AccountGross,
    /// Sum of adverse strategy session-loss contributions exceeds the account cap.
    AccountLoss,
    /// Aggregate open-order count exceeds the account cap.
    AccountOrders,
    /// A strategy's gross exposure/reservations exceeds its own cap.
    StrategyGross(RiskStrategyId),
    /// A strategy's adverse session loss exceeds its own cap.
    StrategyLoss(RiskStrategyId),
    /// A strategy's open-order count exceeds its own cap.
    StrategyOrders(RiskStrategyId),
}
/// Policy result only; a permitted result is not durable account admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RiskDecision {
    /// All implemented limits pass at the supplied revision/time.
    Permitted,
    /// Explicit denial (or the original still-latched breach).
    Denied(RiskReason),
}
