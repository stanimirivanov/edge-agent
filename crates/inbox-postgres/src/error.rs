//! Stable adapter error categories and redacted diagnostics.

use edgeagent_contracts::MessageRoutingError;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable failure categories for inbox callers and acknowledgement policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxErrorKind {
    /// The message is invalid or unsupported by the consumer registry.
    Contract,
    /// One consumer saw different immutable content under the same identity.
    MessageIdentityConflict,
    /// The stable consumer name violates its bounded token contract.
    InvalidConsumerName,
    /// Quarantine identity, bounds, or reason code are invalid.
    InvalidQuarantineEvidence,
    /// One transport delivery key was reused with different immutable evidence.
    QuarantineIdentityConflict,
    /// Replay authorization identity, operator, reason, or target is invalid.
    InvalidReplayRequest,
    /// One replay request identity was reused with different authorization evidence.
    ReplayRequestConflict,
    /// The requested delivery does not exist in inbound quarantine.
    NotQuarantined,
    /// PostgreSQL rejected or could not complete an operation.
    Storage,
    /// Stored columns violate invariants expected by this adapter.
    StorageInvariant,
}

impl Display for InboxErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("inbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("inbox message identity conflicts with stored content")
            }
            Self::InvalidConsumerName => formatter.write_str("inbox consumer name is invalid"),
            Self::InvalidQuarantineEvidence => {
                formatter.write_str("inbox quarantine evidence is invalid")
            }
            Self::QuarantineIdentityConflict => {
                formatter.write_str("inbox quarantine identity conflicts with stored evidence")
            }
            Self::InvalidReplayRequest => {
                formatter.write_str("inbox quarantine replay request is invalid")
            }
            Self::ReplayRequestConflict => formatter
                .write_str("inbox quarantine replay request conflicts with stored evidence"),
            Self::NotQuarantined => formatter.write_str("inbox delivery is not quarantined"),
            Self::Storage => formatter.write_str("inbox storage operation failed"),
            Self::StorageInvariant => formatter.write_str("inbox storage invariant failed"),
        }
    }
}

/// Inbox failure with bounded public text and an optional internal cause.
#[derive(Debug)]
pub struct InboxError {
    kind: InboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl InboxError {
    /// Return the stable category used by handler and acknowledgement policy.
    #[must_use]
    pub const fn kind(&self) -> InboxErrorKind {
        self.kind
    }

    pub(super) const fn invalid_consumer_name(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidConsumerName,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) const fn invalid_quarantine_evidence(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidQuarantineEvidence,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) const fn invalid_replay_request(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidReplayRequest,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) fn storage(error: tokio_postgres::Error) -> Self {
        Self {
            kind: InboxErrorKind::Storage,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    pub(super) const fn storage_invariant() -> Self {
        Self {
            kind: InboxErrorKind::StorageInvariant,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn message_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::MessageIdentityConflict,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn quarantine_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::QuarantineIdentityConflict,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn replay_request_conflict() -> Self {
        Self {
            kind: InboxErrorKind::ReplayRequestConflict,
            reason: None,
            source: None,
        }
    }

    pub(super) const fn not_quarantined() -> Self {
        Self {
            kind: InboxErrorKind::NotQuarantined,
            reason: None,
            source: None,
        }
    }
}

impl Display for InboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for InboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<MessageRoutingError> for InboxError {
    fn from(error: MessageRoutingError) -> Self {
        Self {
            kind: InboxErrorKind::Contract,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}
