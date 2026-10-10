use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, FailureCode, HandlerFailure, HandlerFailureKind,
    InboundProcessingError, InboxStoreError, InboxStoreErrorKind, OutboxStoreError,
    OutboxStoreErrorKind, PublishError, PublishErrorKind,
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

fn assert_category_only<E: Error + Debug>(error: &E, display: &str, debug_kind: &str) {
    assert_eq!(error.to_string(), display);
    let debug = format!("{error:?}");
    assert!(debug.contains(debug_kind), "missing category: {debug}");
    assert!(debug.contains("source_present: false"), "{debug}");
    assert!(!debug.contains(SOURCE_SENTINEL), "{debug}");
    assert!(error.source().is_none());
}

fn assert_classified_cause<E: Error + Debug + 'static>(error: &E, display: &str, debug_kind: &str) {
    assert_redacted(error, display, debug_kind);
    assert_eq!(error.to_string(), display);
    assert!(format!("{error:?}").contains("source_present: true"));
    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<PrivateCause>())
    );
}

#[test]
fn publish_error_constructors_preserve_all_categories_and_actual_source_presence() {
    // Every source-free category must also remain usable in a const context.
    const CASES: [(PublishError, PublishErrorKind, &str); 4] = [
        (
            PublishError::new(PublishErrorKind::Contract),
            PublishErrorKind::Contract,
            "message contract rejected publication",
        ),
        (
            PublishError::new(PublishErrorKind::Unavailable),
            PublishErrorKind::Unavailable,
            "message transport is unavailable",
        ),
        (
            PublishError::new(PublishErrorKind::Rejected),
            PublishErrorKind::Rejected,
            "message transport rejected publication",
        ),
        (
            PublishError::new(PublishErrorKind::ConfirmationUnknown),
            PublishErrorKind::ConfirmationUnknown,
            "message publication confirmation is unknown",
        ),
    ];

    for (error, kind, display) in CASES {
        assert_eq!(error.kind(), kind);
        assert_category_only(&error, display, &format!("{kind:?}"));

        let sourced = PublishError::with_source(kind, PrivateCause);
        assert_eq!(sourced.kind(), kind);
        assert_classified_cause(&sourced, display, &format!("{kind:?}"));
    }
}

#[test]
fn consume_error_constructors_preserve_all_categories_and_actual_source_presence() {
    const CASES: [(ConsumeError, ConsumeErrorKind, &str); 4] = [
        (
            ConsumeError::new(ConsumeErrorKind::Unavailable),
            ConsumeErrorKind::Unavailable,
            "message consumer is unavailable",
        ),
        (
            ConsumeError::new(ConsumeErrorKind::Protocol),
            ConsumeErrorKind::Protocol,
            "message delivery protocol is invalid",
        ),
        (
            ConsumeError::new(ConsumeErrorKind::InvalidDisposition),
            ConsumeErrorKind::InvalidDisposition,
            "message settlement is invalid",
        ),
        (
            ConsumeError::new(ConsumeErrorKind::ConfirmationUnknown),
            ConsumeErrorKind::ConfirmationUnknown,
            "message settlement confirmation is unknown",
        ),
    ];

    for (error, kind, display) in CASES {
        assert_eq!(error.kind(), kind);
        assert_category_only(&error, display, &format!("{kind:?}"));

        let sourced = ConsumeError::with_source(kind, PrivateCause);
        assert_eq!(sourced.kind(), kind);
        assert_classified_cause(&sourced, display, &format!("{kind:?}"));
    }
}

#[test]
fn inbox_error_constructors_preserve_all_categories_and_actual_source_presence() {
    const CASES: [(InboxStoreError, InboxStoreErrorKind, &str); 4] = [
        (
            InboxStoreError::new(InboxStoreErrorKind::Contract),
            InboxStoreErrorKind::Contract,
            "inbox message contract is invalid",
        ),
        (
            InboxStoreError::new(InboxStoreErrorKind::MessageIdentityConflict),
            InboxStoreErrorKind::MessageIdentityConflict,
            "inbox message identity conflicts with stored content",
        ),
        (
            InboxStoreError::new(InboxStoreErrorKind::Unavailable),
            InboxStoreErrorKind::Unavailable,
            "inbox storage is unavailable",
        ),
        (
            InboxStoreError::new(InboxStoreErrorKind::Invariant),
            InboxStoreErrorKind::Invariant,
            "inbox storage invariant failed",
        ),
    ];

    for (error, kind, display) in CASES {
        assert_eq!(error.kind(), kind);
        assert_category_only(&error, display, &format!("{kind:?}"));
        assert!(format!("{error:?}").contains("reason: None"));

        let sourced = InboxStoreError::with_source(kind, PrivateCause);
        assert_eq!(sourced.kind(), kind);
        assert_classified_cause(&sourced, display, &format!("{kind:?}"));
        assert!(format!("{sourced:?}").contains("reason: None"));
    }
}

#[test]
fn outbox_error_constructors_preserve_all_categories_and_actual_source_presence() {
    const CASES: [(OutboxStoreError, OutboxStoreErrorKind, &str); 3] = [
        (
            OutboxStoreError::new(OutboxStoreErrorKind::Unavailable),
            OutboxStoreErrorKind::Unavailable,
            "outbox storage is unavailable",
        ),
        (
            OutboxStoreError::new(OutboxStoreErrorKind::StateTransition),
            OutboxStoreErrorKind::StateTransition,
            "outbox state transition failed",
        ),
        (
            OutboxStoreError::new(OutboxStoreErrorKind::Invariant),
            OutboxStoreErrorKind::Invariant,
            "outbox storage invariant failed",
        ),
    ];

    for (error, kind, display) in CASES {
        assert_eq!(error.kind(), kind);
        assert_category_only(&error, display, &format!("{kind:?}"));
        assert!(format!("{error:?}").contains("reason: None"));

        let sourced = OutboxStoreError::with_source(kind, PrivateCause);
        assert_eq!(sourced.kind(), kind);
        assert_classified_cause(&sourced, display, &format!("{kind:?}"));
        assert!(format!("{sourced:?}").contains("reason: None"));
    }
}

#[test]
fn source_free_store_failure_terminates_the_inbound_processing_source_chain() {
    let error =
        InboundProcessingError::Store(InboxStoreError::new(InboxStoreErrorKind::Unavailable));

    assert_eq!(error.to_string(), "inbox storage is unavailable");
    let debug = format!("{error:?}");
    assert!(debug.contains("Store"), "{debug}");
    assert!(debug.contains("Unavailable"), "{debug}");
    assert!(debug.contains("source_present: false"), "{debug}");
    assert!(!debug.contains(SOURCE_SENTINEL), "{debug}");
    assert!(error.source().is_some_and(|source| {
        let Some(store) = source.downcast_ref::<InboxStoreError>() else {
            return false;
        };
        store.kind() == InboxStoreErrorKind::Unavailable && store.source().is_none()
    }));
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
    let handler = HandlerFailure::with_source(
        HandlerFailureKind::Permanent,
        FailureCode::from_static(CODE_SENTINEL),
        PrivateCause,
    );
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
            FailureCode::from_static("retry"),
            PrivateCause,
        )),
        "message handler failed transiently",
        "Handler",
    );
}
