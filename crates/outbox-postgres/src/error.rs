//! Redacted adapter failures with internal PostgreSQL causes.

use edgeagent_contracts::MessageRoutingError;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

/// Stable failure categories for outbox callers and relay policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxErrorKind {
    /// The message violates its registered contract.
    Contract,
    /// One message identity was reused with different immutable content.
    MessageIdentityConflict,
    /// A bounded adapter argument or replay authorization value is invalid.
    InvalidArgument,
    /// PostgreSQL is unavailable or a transaction can be retried or is ambiguous.
    Storage,
    /// Stored columns, schema, or a deterministic rejection violate adapter invariants.
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
pub struct OutboxError {
    kind: OutboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl Debug for OutboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboxError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
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

    pub(super) fn storage(error: sqlx::Error) -> Self {
        // A deterministic schema or codec defect must not be retried by the
        // relay as if PostgreSQL were temporarily unavailable.
        let kind = match &error {
            sqlx::Error::Database(database) => match database.code() {
                Some(code) if retryable_sqlstate(&code) => OutboxErrorKind::Storage,
                _ => OutboxErrorKind::StorageInvariant,
            },
            sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed
            | sqlx::Error::BeginFailed => OutboxErrorKind::Storage,
            _ => OutboxErrorKind::StorageInvariant,
        };
        Self {
            kind,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    pub(super) const fn storage_invariant(reason: &'static str) -> Self {
        Self {
            kind: OutboxErrorKind::StorageInvariant,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) fn storage_invariant_with_source(
        reason: &'static str,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: OutboxErrorKind::StorageInvariant,
            reason: Some(reason),
            source: Some(Box::new(source)),
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

fn retryable_sqlstate(code: &str) -> bool {
    code.starts_with("08") // connection exception
        || code.starts_with("53") // insufficient resources
        || matches!(
            code,
            "40001" | "40003" | "40P01" // serialization, ambiguity, deadlock
                | "55P03" | "57014" | "57P01" | "57P02" | "57P03"
        )
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

#[cfg(test)]
mod tests;
