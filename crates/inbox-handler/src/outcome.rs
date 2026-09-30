use edgeagent_contracts::{MessageContractError, MessageRoutingError};
use edgeagent_messaging::{HandlerFailure, InboxStoreError, QuarantineDisposition};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// Classified message-processing failure retained in an observable outcome.
#[derive(Debug)]
pub enum MessageFailure {
    /// Structured CloudEvents bytes could not be decoded or validated.
    Envelope(MessageContractError),
    /// The decoded envelope is unsupported or violates its registered route.
    Routing(MessageRoutingError),
    /// Inbox state rejected or could not persist the delivery.
    Inbox(InboxStoreError),
    /// The service-owned domain handler returned a classified failure.
    Handler(HandlerFailure),
}

impl Display for MessageFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope(_) => formatter.write_str("message envelope is invalid"),
            Self::Routing(_) => formatter.write_str("message route is invalid"),
            Self::Inbox(error) => Display::fmt(error, formatter),
            Self::Handler(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MessageFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Envelope(error) => Some(error),
            Self::Routing(error) => Some(error),
            Self::Inbox(error) => Some(error),
            Self::Handler(error) => Some(error),
        }
    }
}

/// Confirmed result of processing one transport delivery.
#[derive(Debug)]
pub enum HandlingOutcome {
    /// A first delivery committed its domain transition and was acknowledged.
    Applied {
        /// One-based transport delivery attempt.
        attempt: u32,
    },
    /// A committed inbox identity suppressed duplicate domain work and was acknowledged.
    Duplicate {
        /// One-based transport delivery attempt.
        attempt: u32,
    },
    /// A transient failure was negatively acknowledged with a bounded delay.
    RetryRequested {
        /// One-based transport delivery attempt.
        attempt: u32,
        /// Deterministic retry delay confirmed by the broker.
        delay: Duration,
        /// Failure that caused redelivery.
        failure: MessageFailure,
    },
    /// Permanent evidence committed and terminal broker settlement was confirmed.
    Quarantined {
        /// One-based transport delivery attempt.
        attempt: u32,
        /// Whether evidence was inserted or already present after redelivery.
        disposition: QuarantineDisposition,
        /// Stable reason retained with the exact payload.
        failure_code: &'static str,
        /// Failure that caused terminal handling.
        failure: MessageFailure,
    },
}
