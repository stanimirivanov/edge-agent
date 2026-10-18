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
            /// Returns `InvalidSign` for a nonpositive price or negative volume,
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
