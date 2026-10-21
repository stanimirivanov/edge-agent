use crate::DeterministicId;
use edgeagent_market_data_domain::{ContractQuantity, OptionPoints, UsdPrice};

/// Payload-free order validation/application failures; no failure authorizes a fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderError {
    /// Missing mode or any value other than the exact literal `dry_run`.
    Mode,
    /// Zero, excessive, mismatched or overflowing quantity.
    Quantity,
    /// Price/quantity units do not match the instrument.
    Unit,
    /// Unsupported option shape, unavailable reference or invalid lifetime.
    Specification,
    /// Regressing time, premature expiry or execution at/after deadline.
    Time,
    /// Retained identity was reused with different immutable content.
    IdentityConflict,
    /// A fact belongs to a different order.
    WrongOrder,
    /// Zero/old/conflicting order revision or exhausted revision counter.
    Revision,
    /// A future revision arrived before its predecessors; defer, do not apply.
    RevisionGap,
    /// Event capacity is invalid or exhausted; do not evict duplicate evidence.
    Capacity,
    /// Lifecycle fact is incompatible with the current state.
    State,
    /// Fill price violates limit protection or option tick rules.
    Price,
    /// Missing matching ready evidence or mismatched run/virtual time.
    Evidence,
    /// Registered simulation model differs from the expected version/configuration.
    ModelMismatch,
    /// Pure simulation computation could not produce a qualified proposal.
    ModelUnavailable,
}
impl std::fmt::Display for OrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dry-run order rejected: {self:?}")
    }
}
impl std::error::Error for OrderError {}

/// Closed dry-run capability. No default, live variant or unchecked constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DryRunMode(());
impl DryRunMode {
    /// Accept only the exact required transport literal, before constructing orders.
    ///
    /// # Errors
    /// Returns `Mode` for absent/unsupported values, including altered case/spacing.
    pub fn parse(mode: Option<&str>) -> Result<Self, OrderError> {
        if mode == Some("dry_run") {
            Ok(Self(()))
        } else {
            Err(OrderError::Mode)
        }
    }
}
macro_rules! identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub(super) [u8; 32]);
        impl $name {
            /// Assign an identity from P05's explicit deterministic ID stream.
            #[must_use]
            pub const fn new(id: DeterministicId) -> Self {
                Self(id.bytes())
            }
            /// Exact namespaced bytes, not credentials or broker identifiers.
            #[must_use]
            pub const fn bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}
identity!(
    OrderId,
    "Stable simulated order identity. Exact submission retries preserve content."
);
identity!(
    OrderEventId,
    "Stable lifecycle fact identity, distinct from its order identity."
);

/// Supported whole-unit order quantities. Fractional shares are not inferred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum QuantityUnit {
    /// Whole equity/ETF shares.
    Shares,
    /// Whole option contracts, not underlying shares.
    Contracts,
}
/// Positive whole quantity with explicit shares/contracts units.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct OrderQuantity {
    pub(super) unit: QuantityUnit,
    pub(super) count: u64,
}
impl OrderQuantity {
    /// Check a whole-share quantity without rounding fractional source volumes.
    ///
    /// # Errors
    /// Returns `Quantity` for zero.
    pub fn shares(count: u64) -> Result<Self, OrderError> {
        Self::with_unit(QuantityUnit::Shares, count)
    }
    /// Preserve a checked whole-contract quantity; never multiply into shares.
    #[must_use]
    pub const fn contracts(count: ContractQuantity) -> Self {
        Self {
            unit: QuantityUnit::Contracts,
            count: count.get(),
        }
    }
    /// Exact unit and count for deterministic arithmetic.
    #[must_use]
    pub const fn parts(self) -> (QuantityUnit, u64) {
        (self.unit, self.count)
    }
    pub(super) fn with_unit(unit: QuantityUnit, count: u64) -> Result<Self, OrderError> {
        if count == 0 {
            return Err(OrderError::Quantity);
        }
        Ok(Self { unit, count })
    }
}
impl std::fmt::Debug for OrderQuantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderQuantity")
            .field("unit", &self.unit)
            .finish_non_exhaustive()
    }
}
/// Price units cannot substitute index points or USD per contract for quotes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderPrice {
    /// Positive USD per share.
    PerShare(UsdPrice),
    /// Nonnegative option premium points, with specification-specific ticks.
    Premium(OptionPoints),
}
/// Long-only intent; sell-to-close still requires P08 inventory reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderSide {
    /// Acquire shares or a long option, subject to funded admission.
    BuyToOpen,
    /// Reduce a reserved long holding; never permission to open a short.
    SellToClose,
}
/// Initial order styles; no implicit marketability, stops or amend/replace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderKind {
    /// Qualified P09 execution price required, with no promised liquidity.
    Market,
    /// Never fill worse than this unit-checked limit.
    Limit(OrderPrice),
}
