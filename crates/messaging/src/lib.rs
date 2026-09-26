//! Application-owned messaging ports and portable publication outcomes.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageDefinition, MessageEnvelope, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;

/// Owned future returned by a message publisher implementation.
pub type PublishFuture<'publisher> =
    Pin<Box<dyn Future<Output = Result<PublishReceipt, PublishError>> + Send + 'publisher>>;

/// Application-owned boundary for durable message publication.
///
/// Implementations MUST derive the transport destination from `definition`,
/// validate `envelope` against that definition, and complete only after the
/// durable transport confirms acceptance or reports a classified failure.
pub trait MessagePublisher: Send + Sync {
    /// Publish one validated envelope using its registered routing definition.
    ///
    /// A caller retrying an unavailable or ambiguous publication MUST preserve
    /// the envelope identity and content. A successful return confirms broker
    /// persistence, not exactly-once domain processing.
    fn publish<'publisher>(
        &'publisher self,
        definition: MessageDefinition,
        envelope: &'publisher MessageEnvelope,
    ) -> PublishFuture<'publisher>;
}

/// Broker-confirmed result of a durable publication attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishReceipt {
    disposition: PublishDisposition,
}

impl PublishReceipt {
    /// Construct a portable receipt from a broker acknowledgement.
    #[must_use]
    pub const fn new(disposition: PublishDisposition) -> Self {
        Self { disposition }
    }

    /// Return whether the broker persisted this attempt or recognized a retry.
    #[must_use]
    pub const fn disposition(self) -> PublishDisposition {
        self.disposition
    }
}

/// Portable disposition reported after durable broker acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishDisposition {
    /// The broker persisted this message as a new entry.
    Persisted,
    /// The broker recognized the stable message identity inside its deduplication window.
    Duplicate,
}

/// Caller-actionable publication failure category shared by broker adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishErrorKind {
    /// The definition or envelope violates the application message contract.
    Contract,
    /// No publish request was accepted; retry the same envelope after backoff.
    Unavailable,
    /// The broker explicitly rejected the request; retry only after remediation.
    Rejected,
    /// The request may have persisted but acknowledgement was lost or unreadable.
    ConfirmationUnknown,
}

impl Display for PublishErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("message contract rejected publication"),
            Self::Unavailable => formatter.write_str("message transport is unavailable"),
            Self::Rejected => formatter.write_str("message transport rejected publication"),
            Self::ConfirmationUnknown => {
                formatter.write_str("message publication confirmation is unknown")
            }
        }
    }
}

/// Publication failure with a stable public category and preserved internal cause.
#[derive(Debug)]
pub struct PublishError {
    kind: PublishErrorKind,
    source: Box<dyn Error + Send + Sync>,
}

impl PublishError {
    /// Wrap an adapter or contract cause without exposing it through `Display`.
    #[must_use]
    pub fn with_source<E>(kind: PublishErrorKind, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            kind,
            source: Box::new(source),
        }
    }

    /// Return the stable category that controls retry and operator behavior.
    #[must_use]
    pub const fn kind(&self) -> PublishErrorKind {
        self.kind
    }
}

impl Display for PublishError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for PublishError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<MessageRoutingError> for PublishError {
    fn from(error: MessageRoutingError) -> Self {
        Self::with_source(PublishErrorKind::Contract, error)
    }
}

#[cfg(test)]
mod tests {
    use super::{PublishError, PublishErrorKind};
    use edgeagent_contracts::MessageRoutingError;
    use std::error::Error;

    #[test]
    fn public_error_is_stable_while_contract_cause_is_preserved() {
        let error = PublishError::from(MessageRoutingError::UnsupportedMessageType(
            "com.edgeagent.research.unknown.v1".to_owned(),
        ));

        assert_eq!(error.kind(), PublishErrorKind::Contract);
        assert_eq!(error.to_string(), "message contract rejected publication");
        assert!(
            error
                .source()
                .is_some_and(|source| source.to_string().contains("unsupported message type"))
        );
    }
}
