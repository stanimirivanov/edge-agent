use super::{OutboxError, OutboxErrorKind, retryable_sqlstate};
use std::error::Error;

#[test]
fn storage_error_debug_omits_driver_cause() {
    let error = OutboxError::storage(sqlx::Error::Io(std::io::Error::other(
        "private-driver-sentinel-7391",
    )));

    assert_eq!(error.kind(), OutboxErrorKind::Storage);
    assert_eq!(error.to_string(), "outbox storage operation failed");
    assert!(!format!("{error:?}").contains("private-driver-sentinel-7391"));
    assert!(error.source().is_some());
}

#[test]
fn sqlx_decode_fault_is_an_invariant_while_transport_loss_is_unavailable() {
    let decode = OutboxError::storage(sqlx::Error::ColumnDecode {
        index: "envelope".to_owned(),
        source: Box::new(std::io::Error::other("private-decode-sentinel")),
    });
    assert_eq!(decode.kind(), OutboxErrorKind::StorageInvariant);
    assert_eq!(decode.to_string(), "outbox storage invariant failed");
    assert!(!format!("{decode:?}").contains("private-decode-sentinel"));

    let unavailable = OutboxError::storage(sqlx::Error::Io(std::io::Error::other(
        "private-connection-sentinel",
    )));
    assert_eq!(unavailable.kind(), OutboxErrorKind::Storage);
    assert!(unavailable.source().is_some());
}

#[test]
fn only_known_postgresql_availability_and_transaction_states_are_retryable() {
    for code in [
        "08006", "40001", "40P01", "40003", "53100", "55P03", "57014",
    ] {
        assert!(retryable_sqlstate(code), "{code} should be retryable");
    }
    for code in ["22001", "23505", "42501", "42703", "42P01", "XX000"] {
        assert!(!retryable_sqlstate(code), "{code} should be invariant");
    }
}

#[test]
fn storage_invariant_without_a_cause_keeps_its_bounded_reason() {
    let error = OutboxError::storage_invariant("claim returned more than one record");

    assert_eq!(error.kind(), OutboxErrorKind::StorageInvariant);
    assert_eq!(
        error.to_string(),
        "outbox storage invariant failed: claim returned more than one record"
    );
    assert!(format!("{error:?}").contains("claim returned more than one record"));
    assert!(error.source().is_none());
}

#[test]
fn storage_invariant_preserves_actual_cause_without_formatting_it() -> Result<(), Box<dyn Error>> {
    let error = OutboxError::storage_invariant_with_source(
        "stored claim violates portable claim invariants",
        std::io::Error::other("private-stored-record-sentinel-8346"),
    );

    assert_eq!(error.kind(), OutboxErrorKind::StorageInvariant);
    assert_eq!(
        error.to_string(),
        "outbox storage invariant failed: stored claim violates portable claim invariants"
    );
    for diagnostic in [error.to_string(), format!("{error:?}")] {
        assert!(!diagnostic.contains("private-stored-record-sentinel-8346"));
    }
    let cause = error
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .ok_or("actual invariant cause must remain inspectable")?;
    assert_eq!(cause.to_string(), "private-stored-record-sentinel-8346");
    Ok(())
}
