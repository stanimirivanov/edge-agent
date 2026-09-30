//! Redacted relay errors with preserved storage causes.

use edgeagent_messaging::{OutboxStoreError, OutboxStoreErrorKind};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable relay-orchestration failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayErrorKind {
    /// Retry or attempt configuration is outside supported bounds.
    InvalidPolicy,
    /// The outbox store was unavailable or could not commit an operation.
    Storage,
    /// The outbox rejected a state transition or returned invalid state.
    Outbox,
}

impl Display for RelayErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("outbox relay policy is invalid"),
            Self::Storage => formatter.write_str("outbox relay storage operation failed"),
            Self::Outbox => formatter.write_str("outbox relay state transition failed"),
        }
    }
}

/// Relay failure with bounded public text and a preserved internal cause.
#[derive(Debug)]
pub struct RelayError {
    kind: RelayErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl RelayError {
    /// Return the stable category used by worker health and restart policy.
    #[must_use]
    pub const fn kind(&self) -> RelayErrorKind {
        self.kind
    }

    pub(super) const fn invalid_policy(reason: &'static str) -> Self {
        Self {
            kind: RelayErrorKind::InvalidPolicy,
            reason: Some(reason),
            source: None,
        }
    }
}

impl Display for RelayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for RelayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<OutboxStoreError> for RelayError {
    fn from(error: OutboxStoreError) -> Self {
        Self {
            kind: match error.kind() {
                OutboxStoreErrorKind::Unavailable => RelayErrorKind::Storage,
                OutboxStoreErrorKind::StateTransition | OutboxStoreErrorKind::Invariant => {
                    RelayErrorKind::Outbox
                }
            },
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}
