//! Deterministic ordering of all evidence-qualified momentum candidates.
use crate::{
    EligibleEvidence,
    momentum::{self, Momentum, MomentumError},
};
use edgeagent_market_data_domain::InstrumentId;
/// One calculated candidate; numeric fields cannot be supplied by the caller.
#[derive(Clone, Copy)]
pub struct RankedMomentum {
    instrument: InstrumentId,
    momentum: Momentum,
}
impl RankedMomentum {
    /// Stable instrument identity, used for exact score ties.
    #[must_use]
    pub const fn instrument(self) -> InstrumentId {
        self.instrument
    }
    /// Versioned calculation including endpoint and lookback metadata.
    #[must_use]
    pub const fn momentum(self) -> Momentum {
        self.momentum
    }
}
/// Rank all qualified candidates by rounded ppm descending, then instrument ascending.
/// Zero and negative scores remain eligible; no strategy threshold is introduced.
/// # Errors
/// Any calculation failure rejects the entire result; no partial ranking escapes.
pub fn rank(evidence: &EligibleEvidence<'_>) -> Result<Vec<RankedMomentum>, MomentumError> {
    let mut ranked = evidence
        .series()
        .iter()
        .map(|s| {
            Ok(RankedMomentum {
                instrument: s.frozen().selected.instrument,
                momentum: momentum::calculate(s)?,
            })
        })
        .collect::<Result<Vec<_>, MomentumError>>()?;
    ranked.sort_by(|a, b| {
        b.momentum
            .score_ppm()
            .cmp(&a.momentum.score_ppm())
            .then_with(|| a.instrument.cmp(&b.instrument))
    });
    Ok(ranked)
}
