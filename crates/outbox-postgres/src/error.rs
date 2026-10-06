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
    /// A bounded lease or retry argument is invalid.
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
mod tests {
    use super::{OutboxError, OutboxErrorKind, retryable_sqlstate};
    use std::error::Error;

    #[test]
    fn storage_error_debug_omits_driver_cause() {
        let error = OutboxError::storage(sqlx::Error::Io(std::io::Error::other(
            "private-driver-sentinel-7391",
        )));

        assert_eq!(error.kind(), OutboxErrorKind::Storage);
        assert_eq!(error.to_string(), "outbox storage operation failed");
        assert!(!format!("{error:?}").contains("private-driver-sentinel-7391"));
        assert!(error.source().is_some());
    }

    #[test]
    fn sqlx_decode_fault_is_an_invariant_while_transport_loss_is_unavailable() {
        let decode = OutboxError::storage(sqlx::Error::ColumnDecode {
            index: "envelope".to_owned(),
            source: Box::new(std::io::Error::other("private-decode-sentinel")),
        });
        assert_eq!(decode.kind(), OutboxErrorKind::StorageInvariant);
        assert_eq!(decode.to_string(), "outbox storage invariant failed");
        assert!(!format!("{decode:?}").contains("private-decode-sentinel"));

        let unavailable = OutboxError::storage(sqlx::Error::Io(std::io::Error::other(
            "private-connection-sentinel",
        )));
        assert_eq!(unavailable.kind(), OutboxErrorKind::Storage);
        assert!(unavailable.source().is_some());
    }

    #[test]
    fn only_known_postgresql_availability_and_transaction_states_are_retryable() {
        for code in [
            "08006", "40001", "40P01", "40003", "53100", "55P03", "57014",
        ] {
            assert!(retryable_sqlstate(code), "{code} should be retryable");
        }
        for code in ["22001", "23505", "42501", "42703", "42P01", "XX000"] {
            assert!(!retryable_sqlstate(code), "{code} should be invariant");
        }
    }
}
