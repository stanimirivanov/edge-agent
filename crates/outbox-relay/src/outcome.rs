//! Confirmed iteration outcomes and bounded quarantine reasons.

use edgeagent_contracts::MessageRoutingError;
use edgeagent_messaging::{FailureCode, PublishDisposition, PublishError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

const STORED_CONTRACT_INVALID: FailureCode = FailureCode::from_static("stored_contract_invalid");
const PUBLISH_CONTRACT: FailureCode = FailureCode::from_static("publish_contract");
const TRANSPORT_REJECTED: FailureCode = FailureCode::from_static("transport_rejected");
const PUBLICATION_BUDGET_EXHAUSTED: FailureCode =
    FailureCode::from_static("publication_budget_exhausted");
const ATTEMPTS_EXHAUSTED_UNAVAILABLE: FailureCode =
    FailureCode::from_static("attempts_exhausted_unavailable");
const ATTEMPTS_EXHAUSTED_CONFIRMATION_UNKNOWN: FailureCode =
    FailureCode::from_static("attempts_exhausted_confirmation_unknown");

/// Observable durable result of one bounded relay iteration.
#[derive(Debug)]
pub enum RelayOutcome {
    /// No eligible outbox record was available.
    Idle,
    /// Broker persistence was confirmed and the outbox record was marked published.
    Published {
        /// Broker acknowledgement disposition.
        disposition: PublishDisposition,
        /// One-based acquired claim count, not the number of network requests.
        attempt: u32,
    },
    /// A transient or ambiguous failure was durably scheduled for retry.
    RetryScheduled {
        /// Publication failure retained for safe diagnostics and cause inspection.
        failure: PublishError,
        /// One-based acquired claim count, not the number of network requests.
        attempt: u32,
        /// Deterministic jittered delay before the next claim.
        delay: Duration,
    },
    /// The retained record entered terminal quarantine and is no longer claimable.
    Quarantined {
        /// Stable, bounded operator reason.
        reason: QuarantineReason,
        /// One-based acquired claim count, not the number of network requests.
        attempt: u32,
        /// Contract, publication, or pre-publication policy failure.
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
    /// The acquired claim exceeded policy; the publisher was not invoked.
    /// Earlier claims may have persisted without an observed result.
    PublicationBudgetExhausted,
}

impl Display for RelayMessageFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoredContract(_) => {
                formatter.write_str("stored outbox message contract is invalid")
            }
            Self::Publication(error) => Display::fmt(error, formatter),
            Self::PublicationBudgetExhausted => {
                formatter.write_str("relay publication claim budget is exhausted")
            }
        }
    }
}

impl Error for RelayMessageFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StoredContract(error) => Some(error),
            Self::Publication(error) => Some(error),
            Self::PublicationBudgetExhausted => None,
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
    /// An acquired claim exceeded policy before publication on that claim.
    PublicationBudgetExhausted,
    /// Transport unavailability on the final permitted claim exhausted policy.
    AttemptsExhaustedUnavailable,
    /// An ambiguous confirmation on the final permitted claim exhausted policy.
    AttemptsExhaustedConfirmationUnknown,
}

impl QuarantineReason {
    /// Return the bounded code persisted for operator inspection.
    #[must_use]
    pub const fn code(self) -> FailureCode {
        match self {
            Self::StoredContractInvalid => STORED_CONTRACT_INVALID,
            Self::PublishContract => PUBLISH_CONTRACT,
            Self::TransportRejected => TRANSPORT_REJECTED,
            Self::PublicationBudgetExhausted => PUBLICATION_BUDGET_EXHAUSTED,
            Self::AttemptsExhaustedUnavailable => ATTEMPTS_EXHAUSTED_UNAVAILABLE,
            Self::AttemptsExhaustedConfirmationUnknown => ATTEMPTS_EXHAUSTED_CONFIRMATION_UNKNOWN,
        }
    }
}
