//! Closed registry of compiled research implementations, not approval authority.
//! Entries accept only sealed eligible evidence. No runtime code or fallback is loaded.
use crate::{
    EligibleEvidence,
    momentum::{self, MomentumError},
    ranking::{self, RankedMomentum},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Implementation {
    Momentum,
}

/// A compiled implementation handle. Availability does not imply strategy approval,
/// verified configuration, authenticated evidence or permission to publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Strategy {
    implementation: Implementation,
}

/// The existing raw-USD momentum entry used by the unchanged M04 application profile.
pub const MOMENTUM: Strategy = Strategy {
    implementation: Implementation::Momentum,
};

/// Resolve an exact compiled formula revision, without aliases or a default fallback.
/// This name is not the request's implementation digest or configuration digest.
#[must_use]
pub fn resolve(revision: &str) -> Option<Strategy> {
    if revision == MOMENTUM.revision() {
        Some(MOMENTUM)
    } else {
        None
    }
}

impl Strategy {
    /// Existing formula revision binding units, basis, rounding and numeric tolerance.
    #[must_use]
    pub const fn revision(self) -> &'static str {
        match self.implementation {
            Implementation::Momentum => momentum::REVISION,
        }
    }

    /// Rank every evidence-qualified candidate using the compiled implementation.
    /// The sole entry preserves momentum's concrete result and error types; a second
    /// real behavior must establish any future shared output abstraction.
    /// # Errors
    /// Returns the original calculation error without partial results or remapping.
    pub fn rank(
        self,
        evidence: &EligibleEvidence<'_>,
    ) -> Result<Vec<RankedMomentum>, MomentumError> {
        match self.implementation {
            Implementation::Momentum => ranking::rank(evidence),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_revision_resolves_the_compiled_momentum_entry() {
        assert_eq!(MOMENTUM.revision(), "raw-usd-roc-ppm-half-even-v1");
        assert_eq!(resolve(momentum::REVISION), Some(MOMENTUM));
    }

    #[test]
    fn unknown_revisions_never_alias_or_fall_back() {
        for revision in [
            "",
            "momentum",
            "latest",
            "raw-usd-roc-ppm-half-even-v2",
            "RAW-USD-ROC-PPM-HALF-EVEN-V1",
            "raw-usd-roc-ppm-half-even-v1 ",
            " raw-usd-roc-ppm-half-even-v1",
            "raw-usd-roc-ppm-half-even-v1\0",
            &"02".repeat(32),
        ] {
            assert_eq!(resolve(revision), None);
        }
    }
}
