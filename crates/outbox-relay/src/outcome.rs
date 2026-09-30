//! Confirmed iteration outcomes and bounded quarantine reasons.

use edgeagent_contracts::MessageRoutingError;
use edgeagent_messaging::{PublishDisposition, PublishError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// Observable durable result of one bounded relay iteration.
#[derive(Debug)]
pub enum RelayOutcome {
    /// No eligible outbox record was available.
    Idle,
    /// Broker persistence was confirmed and the outbox record was marked published.
    Published {
        /// Broker acknowledgement disposition.
        disposition: PublishDisposition,
        /// One-based lease/publication attempt.
        attempt: u32,
    },
    /// A transient or ambiguous failure was durably scheduled for retry.
    RetryScheduled {
        /// Publication failure retained for safe diagnostics and cause inspection.
        failure: PublishError,
        /// One-based lease/publication attempt.
        attempt: u32,
        /// Deterministic jittered delay before the next claim.
        delay: Duration,
    },
    /// The retained record entered terminal quarantine and is no longer claimable.
    Quarantined {
        /// Stable, bounded operator reason.
        reason: QuarantineReason,
        /// One-based lease/publication attempt.
        attempt: u32,
        /// Contract or publication failure retained for diagnostics.
        failure: RelayMessageFailure,
    },
}

/// Handled message failure retained in a durable relay outcome for diagnostics.
#[derive(Debug)]
pub enum RelayMessageFailure {
    /// Stored envelope or denormalized routing metadata failed validation.
    StoredContract(MessageRoutingError),
    /// The portable publisher returned a classified failure.
    Publication(PublishError),
}

impl Display for RelayMessageFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoredContract(_) => {
                formatter.write_str("stored outbox message contract is invalid")
            }
            Self::Publication(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for RelayMessageFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StoredContract(error) => Some(error),
            Self::Publication(error) => Some(error),
        }
    }
}

/// Stable quarantine reasons persisted without broker error text or payload data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineReason {
    /// Stored bytes no longer satisfy the relay's registered contract.
    StoredContractInvalid,
    /// The publisher rejected a definition or envelope contract.
    PublishContract,
    /// The transport explicitly rejected the publication.
    TransportRejected,
    /// Repeated transport unavailability exhausted the attempt budget.
    AttemptsExhaustedUnavailable,
    /// Repeated ambiguous confirmations exhausted the attempt budget.
    AttemptsExhaustedConfirmationUnknown,
}

impl QuarantineReason {
    /// Return the bounded code persisted for operator inspection.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::StoredContractInvalid => "stored_contract_invalid",
            Self::PublishContract => "publish_contract",
            Self::TransportRejected => "transport_rejected",
            Self::AttemptsExhaustedUnavailable => "attempts_exhausted_unavailable",
            Self::AttemptsExhaustedConfirmationUnknown => "attempts_exhausted_confirmation_unknown",
        }
    }
}
