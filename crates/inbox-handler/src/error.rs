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
pub struct HandlerError {
    kind: HandlerErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl std::fmt::Debug for HandlerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandlerError")
            .field("kind", &self.kind)
            .field("reason", &self.reason)
            .field("source_present", &self.source.is_some())
            .finish()
    }
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

#[cfg(test)]
mod tests {
    use super::HandlerError;
    use crate::{HandlingOutcome, MessageFailure};
    use edgeagent_messaging::{
        ConsumeError, ConsumeErrorKind, HandlerFailure, HandlerFailureKind, InboxStoreError,
        InboxStoreErrorKind,
    };
    use std::error::Error;
    use std::fmt::{Debug, Display, Formatter};
    use std::time::Duration;

    const SENTINEL: &str = "private-handler-source-sentinel-7391";

    struct PrivateCause;

    impl Debug for PrivateCause {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_tuple("PrivateCause")
                .field(&SENTINEL)
                .finish()
        }
    }

    impl Display for PrivateCause {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(SENTINEL)
        }
    }

    impl Error for PrivateCause {}

    fn assert_redacted<E: Error + Debug + 'static>(error: &E) {
        assert!(!error.to_string().contains(SENTINEL));
        assert!(!format!("{error:?}").contains(SENTINEL));
        let mut cause: &dyn Error = error;
        while let Some(source) = cause.source() {
            cause = source;
        }
        assert!(cause.is::<PrivateCause>());
        assert_eq!(cause.to_string(), SENTINEL);
    }

    #[test]
    fn coordinator_errors_and_outcomes_redact_nested_causes() {
        assert_redacted(&HandlerError::inbox(InboxStoreError::with_source(
            InboxStoreErrorKind::Unavailable,
            PrivateCause,
        )));
        assert_redacted(&HandlerError::settlement(ConsumeError::with_source(
            ConsumeErrorKind::ConfirmationUnknown,
            PrivateCause,
        )));

        let outcome = HandlingOutcome::RetryRequested {
            attempt: 1,
            delay: Duration::from_millis(1),
            failure: MessageFailure::Handler(HandlerFailure::with_source(
                HandlerFailureKind::Transient,
                "retry",
                PrivateCause,
            )),
        };
        assert!(!format!("{outcome:?}").contains(SENTINEL));
    }
}
