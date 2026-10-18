use std::num::NonZero;

use crate::ValueError;

// Only construction/access boilerplate is shared; namespaces stay distinct.
macro_rules! identity {
    ($name:ident, $repr:ty, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(NonZero<$repr>);

        impl $name {
            /// Accept a nonzero, caller-assigned identity; performs no generation or I/O.
            ///
            /// # Errors
            /// Returns `ValueError::ZeroIdentity` for zero.
            pub fn new(value: $repr) -> Result<Self, ValueError> {
                NonZero::new(value)
                    .map(Self)
                    .ok_or(ValueError::ZeroIdentity)
            }

            /// Return the opaque value; it is not a provider symbol or wire format.
            #[must_use]
            pub const fn get(self) -> $repr {
                self.0.get()
            }
        }
    };
}

identity!(
    InstrumentId,
    u128,
    "Service-owned instrument identity, independent of ticker or listing."
);
identity!(
    ListingId,
    u128,
    "Service-owned listing identity; distinct from instrument identity."
);
identity!(
    SourceId,
    u128,
    "Identity of a provenance source in the service-owned catalog."
);
identity!(
    TransformationRevision,
    u64,
    "Service-owned transformation revision; not a vendor revision claim."
);
