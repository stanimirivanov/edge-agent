use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, HandlerFailure, HandlerFailureKind, InboundProcessingError,
    InboxStoreError, InboxStoreErrorKind, OutboxStoreError, OutboxStoreErrorKind, PublishError,
    PublishErrorKind,
};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

const SOURCE_SENTINEL: &str = "private-adapter-source-sentinel-7391";
const CODE_SENTINEL: &str = "private-handler-code-sentinel-7391";

struct PrivateCause;

impl Debug for PrivateCause {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("PrivateCause")
            .field(&SOURCE_SENTINEL)
            .finish()
    }
}

impl Display for PrivateCause {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(SOURCE_SENTINEL)
    }
}

impl Error for PrivateCause {}

fn assert_redacted<E: Error + Debug + 'static>(
    error: &E,
    display_category: &str,
    debug_category: &str,
) {
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(
        display.contains(display_category),
        "missing category: {display}"
    );
    assert!(debug.contains(debug_category), "missing category: {debug}");
    assert!(!display.contains(SOURCE_SENTINEL), "{display}");
    assert!(!debug.contains(SOURCE_SENTINEL), "{debug}");

    let mut cause: &dyn Error = error;
    while let Some(source) = cause.source() {
        cause = source;
    }
    assert!(cause.is::<PrivateCause>());
    assert_eq!(cause.to_string(), SOURCE_SENTINEL);
    assert!(format!("{cause:?}").contains(SOURCE_SENTINEL));
}

#[test]
fn public_messaging_errors_redact_causes_but_preserve_source_chains() {
    assert_redacted(
        &ConsumeError::with_source(ConsumeErrorKind::Protocol, PrivateCause),
        "message delivery protocol is invalid",
        "Protocol",
    );
    assert_redacted(
        &PublishError::with_source(PublishErrorKind::Unavailable, PrivateCause),
        "message transport is unavailable",
        "Unavailable",
    );
    let handler =
        HandlerFailure::with_source(HandlerFailureKind::Permanent, CODE_SENTINEL, PrivateCause);
    assert_redacted(&handler, "message handler rejected", "Permanent");
    assert!(!format!("{handler:?}").contains(CODE_SENTINEL));
    assert_redacted(
        &InboxStoreError::with_source(InboxStoreErrorKind::Unavailable, PrivateCause),
        "inbox storage is unavailable",
        "Unavailable",
    );
    assert_redacted(
        &OutboxStoreError::with_source(OutboxStoreErrorKind::StateTransition, PrivateCause),
        "outbox state transition failed",
        "StateTransition",
    );
    assert_redacted(
        &InboundProcessingError::Store(InboxStoreError::with_source(
            InboxStoreErrorKind::Unavailable,
            PrivateCause,
        )),
        "inbox storage is unavailable",
        "Store",
    );
    assert_redacted(
        &InboundProcessingError::Handler(HandlerFailure::with_source(
            HandlerFailureKind::Transient,
            "retry",
            PrivateCause,
        )),
        "message handler failed transiently",
        "Handler",
    );
}
