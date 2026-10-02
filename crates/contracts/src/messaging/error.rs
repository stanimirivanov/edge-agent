//! Payload-safe contract error categories and parser coordinates.

use std::error::Error;
use std::fmt::{Display, Formatter};
/// Validation or encoding failure at the EdgeAgent message boundary.
///
/// Serde and CloudEvents diagnostic text may include caller-controlled data.
/// This error retains only stable categories, safe field names, byte counts,
/// and parser coordinates; it does not expose the underlying diagnostic as a
/// source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageContractError {
    /// One required metadata value violates a stable invariant.
    InvalidMetadata {
        /// CloudEvents or EdgeAgent extension field name.
        field: &'static str,
        /// Stable, non-sensitive explanation.
        reason: &'static str,
    },
    /// Payload-to-JSON serialization failed; serializer text is withheld.
    PayloadEncoding,
    /// JSON-to-payload deserialization failed; decoder text is withheld.
    PayloadDecoding,
    /// CloudEvents construction failed; SDK text is withheld.
    CloudEvent,
    /// Structured JSON serialization failed; serializer text is withheld.
    EnvelopeEncoding,
    /// Structured JSON parsing failed; only safe parser coordinates are retained.
    EnvelopeDecoding {
        /// Parser line, when available (zero for semantic errors).
        line: usize,
        /// Parser column, when available (zero for semantic errors).
        column: usize,
    },
    /// Raw structured envelope exceeds the portable byte limit.
    EnvelopeTooLarge {
        /// Actual raw input length in bytes.
        actual_bytes: usize,
        /// Maximum permitted raw input length in bytes.
        maximum_bytes: usize,
    },
}

impl Display for MessageContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetadata { field, reason } => {
                write!(formatter, "invalid {field}: {reason}")
            }
            Self::PayloadEncoding => formatter.write_str("cannot encode payload"),
            Self::PayloadDecoding => formatter.write_str("cannot decode payload"),
            Self::CloudEvent => formatter.write_str("cannot build CloudEvent"),
            Self::EnvelopeEncoding => formatter.write_str("cannot encode CloudEvent envelope"),
            Self::EnvelopeDecoding { line, column } => write!(
                formatter,
                "cannot decode CloudEvent envelope at line {line}, column {column}"
            ),
            Self::EnvelopeTooLarge {
                actual_bytes,
                maximum_bytes,
            } => write!(
                formatter,
                "CloudEvent envelope is {actual_bytes} bytes; maximum is {maximum_bytes} bytes"
            ),
        }
    }
}

impl Error for MessageContractError {}

pub(super) fn envelope_decoding(error: serde_json::Error) -> MessageContractError {
    MessageContractError::EnvelopeDecoding {
        line: error.line(),
        column: error.column(),
    }
}
