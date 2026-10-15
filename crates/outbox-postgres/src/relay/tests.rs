use super::{OutboxStoreErrorKind, PostgresOutboxRelay, database, operation};
use crate::OutboxError;
use edgeagent_contracts::MessageRoutingError;
use std::error::Error;

#[test]
fn adapter_preserves_all_error_categories() {
    let cases = [
        (
            OutboxError::from(MessageRoutingError::ContractMismatch { field: "type" }),
            OutboxStoreErrorKind::Invariant,
        ),
        (
            OutboxError::message_identity_conflict(),
            OutboxStoreErrorKind::Invariant,
        ),
        (
            OutboxError::invalid_argument("lease_owner is invalid"),
            OutboxStoreErrorKind::Invariant,
        ),
        (
            OutboxError::storage(sqlx::Error::Io(std::io::Error::other("transport fault"))),
            OutboxStoreErrorKind::Unavailable,
        ),
        (
            OutboxError::storage(sqlx::Error::RowNotFound),
            OutboxStoreErrorKind::Invariant,
        ),
        (
            OutboxError::lease_lost(),
            OutboxStoreErrorKind::StateTransition,
        ),
        (
            OutboxError::replay_request_conflict(),
            OutboxStoreErrorKind::Invariant,
        ),
        (
            OutboxError::not_quarantined(),
            OutboxStoreErrorKind::Invariant,
        ),
    ];

    for (error, expected) in cases {
        let original = error.kind();
        assert_eq!(operation(error).kind(), expected, "{original:?}");
    }
}

#[test]
fn transaction_driver_failures_use_the_operation_classifier_and_preserve_causes()
-> Result<(), Box<dyn Error>> {
    const SENTINEL: &str = "private-relay-driver-sentinel-8346";
    let cases = [
        (
            sqlx::Error::ColumnDecode {
                index: "envelope".to_owned(),
                source: Box::new(std::io::Error::other(SENTINEL)),
            },
            OutboxStoreErrorKind::Invariant,
        ),
        (
            sqlx::Error::Io(std::io::Error::other(SENTINEL)),
            OutboxStoreErrorKind::Unavailable,
        ),
    ];

    for (cause, expected) in cases {
        let error = database(cause);
        assert_eq!(error.kind(), expected);
        for diagnostic in [error.to_string(), format!("{error:?}")] {
            assert!(!diagnostic.contains(SENTINEL));
        }
        let adapter_error = error
            .source()
            .and_then(|source| source.downcast_ref::<OutboxError>())
            .ok_or("adapter error must remain inspectable")?;
        let driver_error = adapter_error
            .source()
            .and_then(|source| source.downcast_ref::<sqlx::Error>())
            .ok_or("driver error must remain inspectable")?;
        assert!(matches!(
            driver_error,
            sqlx::Error::ColumnDecode { .. } | sqlx::Error::Io(_)
        ));
    }
    Ok(())
}

#[test]
fn adapter_supports_safe_debug_and_send() {
    fn assert_traits<T: std::fmt::Debug + Send>() {}
    assert_traits::<PostgresOutboxRelay<'static>>();
}
