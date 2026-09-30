use edgeagent_messaging::{ConsumeError, InboxStoreError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Stable consumer-orchestration failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerErrorKind {
    /// Consumer identity or retry configuration is invalid.
    InvalidPolicy,
    /// A domain handler returned an unsafe reason code.
    InvalidHandlerFailure,
    /// Inbox state violates a non-retriable invariant.
    Inbox,
    /// Broker settlement failed or its confirmation is unknown.
    Settlement,
}

impl Display for HandlerErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("message handler policy is invalid"),
            Self::InvalidHandlerFailure => {
                formatter.write_str("message handler failure classification is invalid")
            }
            Self::Inbox => formatter.write_str("message handler inbox transition failed"),
            Self::Settlement => formatter.write_str("message delivery settlement failed"),
        }
    }
}

/// Coordinator failure with bounded public text and a preserved internal cause.
#[derive(Debug)]
pub struct HandlerError {
    kind: HandlerErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl HandlerError {
    /// Return the stable category used by worker health and restart policy.
    #[must_use]
    pub const fn kind(&self) -> HandlerErrorKind {
        self.kind
    }

    pub(super) const fn invalid_policy(reason: &'static str) -> Self {
        Self {
            kind: HandlerErrorKind::InvalidPolicy,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) const fn invalid_handler_failure(reason: &'static str) -> Self {
        Self {
            kind: HandlerErrorKind::InvalidHandlerFailure,
            reason: Some(reason),
            source: None,
        }
    }

    pub(super) fn inbox(error: InboxStoreError) -> Self {
        Self {
            kind: HandlerErrorKind::Inbox,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    pub(super) fn settlement(error: ConsumeError) -> Self {
        Self {
            kind: HandlerErrorKind::Settlement,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

impl Display for HandlerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for HandlerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
