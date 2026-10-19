use std::fmt;

use crate::ValueError;

/// Supported quotation currency; expanding coverage requires a profile revision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Currency {
    /// United States dollar.
    Usd,
}

macro_rules! quantity {
    ($name:ident, $scale:literal, $minimum:literal, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(i128);

        impl $name {
            /// Construct exactly as `coefficient * 10^-scale`, without rounding.
            /// Excess scale is rejected even if fractional trailing zeros could be removed.
            ///
            /// # Errors
            /// Returns `InvalidSign` below this unit's minimum (positive or nonnegative),
            /// `Scale` for excess fractional scale, or `Precision` when the
            /// normalized coefficient exceeds 28 digits.
            pub fn new(coefficient: i128, scale: u32) -> Result<Self, ValueError> {
                if coefficient < $minimum {
                    return Err(ValueError::InvalidSign);
                }
                if scale > $scale {
                    return Err(ValueError::Scale);
                }
                let (coefficient, scale) = normalize(coefficient, scale);
                if coefficient >= 10_i128.pow(28) {
                    return Err(ValueError::Precision);
                }
                // At most 28 coefficient digits plus 8 scale digits fit in i128.
                coefficient
                    .checked_mul(10_i128.pow($scale - scale))
                    .map(Self)
                    .ok_or(ValueError::Precision)
            }

            /// Canonical coefficient/scale pair, suitable for exact constructor round trips.
            #[must_use]
            pub fn decimal_parts(self) -> (i128, u32) {
                normalize(self.0, $scale)
            }
        }

        // Prices/volumes may be licensed evidence; diagnostics must not print values.
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(..)"))
            }
        }
    };
}

quantity!(
    UsdPrice,
    8,
    1,
    "Positive USD per share with at most eight fractional decimal places."
);
quantity!(
    ShareVolume,
    6,
    0,
    "Nonnegative reported shares with at most six fractional decimal places."
);
quantity!(
    OptionPoints,
    8,
    0,
    "Nonnegative option quotation/payoff points, not USD per share or total cash."
);
quantity!(
    IndexPoints,
    8,
    1,
    "Positive non-tradable cash-index points; not a share price or option premium."
);

/// Nonnegative displayed whole contracts; zero is known empty, not unknown size.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisplayedContracts(u64);
impl DisplayedContracts {
    /// Preserve an exact displayed count; imposes no liquidity or fill guarantee.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Explicit count access; `None` at a quote side means unknown instead.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl fmt::Debug for DisplayedContracts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DisplayedContracts(..)")
    }
}
quantity!(
    UsdPerPoint,
    8,
    1,
    "Positive USD per option point per contract; no implicit 100-share multiplier."
);

impl OptionPoints {
    /// Whether points lie on a positive zero-origin premium tick grid. Variable
    /// tick schedules require separate qualification; zero increment never matches.
    #[must_use]
    pub fn is_multiple_of(self, increment: Self) -> bool {
        increment.0 > 0 && self.0 % increment.0 == 0
    }
}

/// Positive whole contracts; deliberately not interchangeable with share volume.
///
/// ```compile_fail,E0308
/// use edgeagent_market_data_domain::{ContractQuantity, ShareVolume};
/// fn contracts(_: ContractQuantity) {}
/// fn invalid(shares: ShareVolume) { contracts(shares); }
/// ```
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContractQuantity(std::num::NonZeroU64);
impl ContractQuantity {
    /// Accept a positive whole number of contracts without rounding.
    ///
    /// # Errors
    /// Returns `ValueError::InvalidSign` for zero.
    pub fn new(value: u64) -> Result<Self, ValueError> {
        std::num::NonZeroU64::new(value)
            .map(Self)
            .ok_or(ValueError::InvalidSign)
    }
    /// Return whole contracts; apply specification multipliers explicitly later.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}
impl fmt::Debug for ContractQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContractQuantity(..)")
    }
}

impl UsdPrice {
    /// The unit binds this value to USD; it cannot carry a mismatched currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        Currency::Usd
    }
}

fn normalize(mut coefficient: i128, mut scale: u32) -> (i128, u32) {
    while scale > 0 && coefficient % 10 == 0 {
        coefficient /= 10;
        scale -= 1;
    }
    (coefficient, scale)
}
