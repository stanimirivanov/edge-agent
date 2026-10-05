//! Stable adapter error categories and redacted diagnostics.

use edgeagent_contracts::MessageRoutingError;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

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
    /// PostgreSQL is unavailable, a transaction can be retried, or its outcome is ambiguous.
    Storage,
    /// Stored columns, schema, or a deterministic database rejection violate adapter invariants.
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
pub struct InboxError {
    kind: InboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl Debug for InboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InboxError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
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

    pub(super) fn storage(error: sqlx::Error) -> Self {
        // Retry only known server availability and transaction states. SQLx
        // also distinguishes local encode/decode defects from transport loss.
        let kind = match &error {
            sqlx::Error::Database(database) => match database.code() {
                Some(code) if retryable_sqlstate(&code) => InboxErrorKind::Storage,
                _ => InboxErrorKind::StorageInvariant,
            },
            sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed
            | sqlx::Error::BeginFailed => InboxErrorKind::Storage,
            _ => InboxErrorKind::StorageInvariant,
        };
        Self {
            kind,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    pub(super) fn ambiguous_storage(error: sqlx::Error) -> Self {
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

fn retryable_sqlstate(code: &str) -> bool {
    code.starts_with("08") // connection exception
        || code.starts_with("53") // insufficient resources
        || matches!(
            code,
            "40001" | "40003" | "40P01" // serialization, ambiguity, deadlock
                | "55P03" | "57014" | "57P01" | "57P02" | "57P03"
        )
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

#[cfg(test)]
mod tests {
    use super::{InboxError, InboxErrorKind, retryable_sqlstate};
    use std::error::Error;

    #[test]
    fn only_known_availability_and_transaction_states_are_retryable() {
        for code in [
            "08006", "40001", "40P01", "40003", "53100", "53300", "55P03", "57014", "57P01",
        ] {
            assert!(retryable_sqlstate(code), "{code} should be retryable");
        }
        for code in [
            "22001", "23505", "40002", "42501", "42703", "42P01", "XX000",
        ] {
            assert!(!retryable_sqlstate(code), "{code} should be invariant");
        }
    }

    #[test]
    fn sqlx_codec_faults_fail_closed_while_transport_loss_remains_unavailable() {
        let decode = InboxError::storage(sqlx::Error::ColumnDecode {
            index: "payload".to_owned(),
            source: Box::new(std::io::Error::other("private-decode-sentinel")),
        });
        assert_eq!(decode.kind(), InboxErrorKind::StorageInvariant);
        assert_eq!(decode.to_string(), "inbox storage invariant failed");
        assert!(!format!("{decode:?}").contains("private-decode-sentinel"));
        assert!(decode.source().is_some());

        let unavailable = InboxError::storage(sqlx::Error::Io(std::io::Error::other(
            "private-connection-sentinel",
        )));
        assert_eq!(unavailable.kind(), InboxErrorKind::Storage);
        assert_eq!(unavailable.to_string(), "inbox storage operation failed");
        assert!(!format!("{unavailable:?}").contains("private-connection-sentinel"));
        assert!(unavailable.source().is_some());
    }

    #[test]
    fn debug_redacts_internal_database_cause_but_preserves_error_chain() {
        let error = InboxError {
            kind: InboxErrorKind::StorageInvariant,
            reason: None,
            source: Some(Box::new(std::io::Error::other(
                "private-database-sentinel-7391",
            ))),
        };

        let rendered = format!("{error:?}");
        assert!(!rendered.contains("private-database-sentinel-7391"));
        assert!(rendered.contains("StorageInvariant"));
        assert!(error.source().is_some());
    }
}
