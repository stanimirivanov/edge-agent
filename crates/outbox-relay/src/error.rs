//! Runtime classifications with independent primary and triggering failures.

use crate::RelayMessageFailure;
use edgeagent_messaging::{OutboxStoreError, OutboxStoreErrorKind};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Observed runtime failure categories, never startup configuration errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayErrorKind {
    /// Storage was unavailable or its commit could not be confirmed.
    Unavailable,
    /// Storage rejected the transition; not necessarily lease loss.
    StateTransition,
    /// Storage returned invalid state, or a calculation violated bounds.
    Invariant,
}
impl Display for RelayErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "outbox relay storage is unavailable",
            Self::StateTransition => "outbox relay state transition was rejected",
            Self::Invariant => "outbox relay invariant failed",
        })
    }
}

/// Runtime failure with a primary cause and optional earlier message failure.
///
/// [`Error::source`] preserves the primary storage/calculation cause. A failed
/// retry/quarantine additionally retains its trigger through
/// [`Self::triggering_failure`], never by replacing the primary source chain.
/// Formatting reveals neither chain; explicit inspection must redact sensitive
/// causes before logging. Confirmed publication is not a failure, so a failed
/// published-marker transition has no triggering message failure.
pub struct RelayError {
    kind: RelayErrorKind,
    source: Box<dyn Error + Send + Sync>,
    triggering_failure: Option<RelayMessageFailure>,
}
impl std::fmt::Debug for RelayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayError")
            .field("kind", &self.kind)
            .field(
                "triggering_failure_present",
                &self.triggering_failure.is_some(),
            )
            .finish_non_exhaustive()
    }
}
impl RelayError {
    /// Return the observed category for worker health and recovery policy.
    #[must_use]
    pub const fn kind(&self) -> RelayErrorKind {
        self.kind
    }
    /// Inspect the earlier failure whose retry/quarantine could not complete.
    ///
    /// Absent for claim or confirmed-publication marker errors. This context's
    /// source may contain private adapter diagnostics; redact before logging.
    #[must_use]
    pub const fn triggering_failure(&self) -> Option<&RelayMessageFailure> {
        self.triggering_failure.as_ref()
    }
    pub(super) fn with_triggering_failure(mut self, failure: RelayMessageFailure) -> Self {
        self.triggering_failure = Some(failure);
        self
    }
    pub(super) fn calculation_invariant(source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            kind: RelayErrorKind::Invariant,
            source: Box::new(source),
            triggering_failure: None,
        }
    }
}
impl Display for RelayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}
impl Error for RelayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}
impl From<OutboxStoreError> for RelayError {
    fn from(error: OutboxStoreError) -> Self {
        Self {
            kind: match error.kind() {
                OutboxStoreErrorKind::Unavailable => RelayErrorKind::Unavailable,
                OutboxStoreErrorKind::StateTransition => RelayErrorKind::StateTransition,
                OutboxStoreErrorKind::Invariant => RelayErrorKind::Invariant,
            },
            source: Box::new(error),
            triggering_failure: None,
        }
    }
}
#[cfg(test)]
mod tests;
