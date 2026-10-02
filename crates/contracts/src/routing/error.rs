//! Payload-safe routing error categories.

use crate::MessageContractError;
use std::error::Error;
use std::fmt::{Display, Formatter};
/// Routing definition or envelope eligibility failure.
///
/// Diagnostic variants do not retain message-provided values. Inspect the
/// quarantined envelope through the authorized evidence path when needed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageRoutingError {
    /// A static routing definition is internally invalid.
    InvalidDefinition {
        /// Invalid definition field.
        field: &'static str,
        /// Stable explanation.
        reason: &'static str,
    },
    /// Two definitions declare the same message type; the value is withheld.
    DuplicateMessageType,
    /// Two definitions resolve to the same transport subject; the value is withheld.
    DuplicateSubject,
    /// No definition accepts the envelope's exact major-version type.
    UnsupportedMessageType,
    /// Envelope metadata conflicts with its registered definition.
    ContractMismatch {
        /// Mismatching envelope field.
        field: &'static str,
    },
    /// Encoded structured message exceeds the portable limit.
    EnvelopeTooLarge {
        /// Actual encoded length.
        actual_bytes: usize,
        /// Maximum portable length.
        maximum_bytes: usize,
    },
    /// Envelope construction or encoding failed.
    Envelope(MessageContractError),
}

impl Display for MessageRoutingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDefinition { field, reason } => {
                write!(formatter, "invalid message definition {field}: {reason}")
            }
            Self::DuplicateMessageType => formatter.write_str("duplicate message type"),
            Self::DuplicateSubject => formatter.write_str("duplicate subject"),
            Self::UnsupportedMessageType => formatter.write_str("unsupported message type"),
            Self::ContractMismatch { field } => write!(formatter, "message {field} mismatch"),
            Self::EnvelopeTooLarge {
                actual_bytes,
                maximum_bytes,
            } => write!(
                formatter,
                "message envelope is {actual_bytes} bytes; portable maximum is {maximum_bytes}"
            ),
            Self::Envelope(error) => write!(formatter, "message envelope error: {error}"),
        }
    }
}

impl Error for MessageRoutingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Envelope(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MessageContractError> for MessageRoutingError {
    fn from(error: MessageContractError) -> Self {
        Self::Envelope(error)
    }
}
