use std::{fmt, num::NonZeroU64};

use crate::{
    InstrumentId, ReferenceError, ReferenceHistory, ReferenceVersion, SessionDate, UsdPrice,
    UtcInstant,
};

/// Corporate-action annotations only; no variant authorizes adjusted prices.
#[derive(Clone, Copy, Eq, PartialEq)]
#[non_exhaustive]
pub enum ActionKind {
    /// Exact old/new share counts; supports forward and reverse splits without floats.
    Split {
        /// Positive shares before the split.
        before: NonZeroU64,
        /// Positive shares after the split.
        after: NonZeroU64,
    },
    /// Positive USD cash amount per share; no reinvestment or return calculation.
    CashDividend(UsdPrice),
    /// Unsupported reorganization or unresolved identity continuity; must abstain.
    Unsupported,
    /// A later revision withdraws this logical action, without deleting its history.
    Cancelled,
}
impl fmt::Debug for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Split { .. } => "Split(..)",
            Self::CashDividend(_) => "CashDividend(..)",
            Self::Unsupported => "Unsupported",
            Self::Cancelled => "Cancelled",
        })
    }
}

/// Action effective date is separate from the enclosing version's receipt time.
/// A future effective date can legitimately be known today.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorporateAction {
    instrument: InstrumentId,
    effective: SessionDate,
    kind: ActionKind,
}
impl CorporateAction {
    /// Record checked metadata, never transform a price or splice listing histories.
    #[must_use]
    pub const fn new(instrument: InstrumentId, effective: SessionDate, kind: ActionKind) -> Self {
        Self {
            instrument,
            effective,
            kind,
        }
    }
    /// Stable instrument and effective date (split execution or dividend ex-date).
    #[must_use]
    pub const fn key(self) -> (InstrumentId, SessionDate) {
        (self.instrument, self.effective)
    }
    /// Exact annotation; diagnostics redact numeric terms.
    #[must_use]
    pub const fn kind(self) -> ActionKind {
        self.kind
    }
}
impl ReferenceHistory<CorporateAction> {
    /// Return known, unwithdrawn annotations effective on this date, in record-ID
    /// order. Empty does NOT prove complete action coverage; ingestion must attest
    /// completeness before evidence acceptance. Preserve unsupported observations.
    ///
    /// # Errors
    /// Returns `UnsupportedAction` if any selected action requires unsupported handling.
    pub fn actions_on(
        &self,
        instrument: InstrumentId,
        date: SessionDate,
        cutoff: UtcInstant,
    ) -> Result<Vec<&ReferenceVersion<CorporateAction>>, ReferenceError> {
        let mut actions = Vec::new();
        for version in self
            .at(cutoff)
            .filter(|v| v.value().key() == (instrument, date))
        {
            match version.value().kind {
                ActionKind::Unsupported => return Err(ReferenceError::UnsupportedAction),
                ActionKind::Cancelled => {}
                _ => actions.push(version),
            }
        }
        Ok(actions)
    }
}
