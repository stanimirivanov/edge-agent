//! Redacted adapter failures with internal PostgreSQL causes.

use edgeagent_contracts::MessageRoutingError;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable failure categories for outbox callers and relay policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxErrorKind {
    /// The message violates its registered contract.
    Contract,
    /// One message identity was reused with different immutable content.
    MessageIdentityConflict,
    /// A bounded lease or retry argument is invalid.
    InvalidArgument,
    /// PostgreSQL rejected or could not complete an operation.
    Storage,
    /// Stored columns violate invariants expected by this adapter.
    StorageInvariant,
    /// A completion attempted to use an absent, expired, or foreign lease.
    LeaseLost,
    /// One replay request identity was reused with different authorization evidence.
    ReplayRequestConflict,
    /// The requested record does not exist in outbound quarantine.
    NotQuarantined,
}

impl Display for OutboxErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("outbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("outbox message identity conflicts with stored content")
            }
            Self::InvalidArgument => formatter.write_str("outbox argument is invalid"),
            Self::Storage => formatter.write_str("outbox storage operation failed"),
            Self::StorageInvariant => formatter.write_str("outbox storage invariant failed"),
            Self::LeaseLost => formatter.write_str("outbox relay lease was lost"),
            Self::ReplayRequestConflict => {
                formatter.write_str("outbox replay request conflicts with stored audit evidence")
            }
            Self::NotQuarantined => {
                formatter.write_str("outbox message is not available for quarantine replay")
            }
        }
    }
}

/// Outbox failure with bounded public text and an optional internal cause.
#[derive(Debug)]
pub struct OutboxError {
    kind: OutboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl OutboxError {
    /// Return the stable category used by application and relay policy.
    #[must_use]
    pub const fn kind(&self) -> OutboxErrorKind {
        self.kind
    }

    pub(super) const fn invalid_argument(reason: &'static str) -> Self {
        Self {
            kind: OutboxErrorKind::InvalidArgument,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) fn storage(error: tokio_postgres::Error) -> Self {
        Self {
            kind: OutboxErrorKind::Storage,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    pub(super) const fn storage_invariant() -> Self {
        Self {
            kind: OutboxErrorKind::StorageInvariant,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn message_identity_conflict() -> Self {
        Self {
            kind: OutboxErrorKind::MessageIdentityConflict,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn lease_lost() -> Self {
        Self {
            kind: OutboxErrorKind::LeaseLost,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn replay_request_conflict() -> Self {
        Self {
            kind: OutboxErrorKind::ReplayRequestConflict,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn not_quarantined() -> Self {
        Self {
            kind: OutboxErrorKind::NotQuarantined,
            reason: None,
            source: None,
        }
    }
}

impl Display for OutboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for OutboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<MessageRoutingError> for OutboxError {
    fn from(error: MessageRoutingError) -> Self {
        Self {
            kind: OutboxErrorKind::Contract,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}
