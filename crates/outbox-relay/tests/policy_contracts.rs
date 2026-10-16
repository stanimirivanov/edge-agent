//! Credential-free public startup/runtime contract regressions.

use edgeagent_messaging::{LeaseDuration, OutboxStoreError, OutboxStoreErrorKind};
use edgeagent_outbox_relay::{
    MaxAttempts, RelayError, RelayErrorKind, RelayPolicy, RelayPolicyError, RetryBackoff,
};
use std::error::Error;
use std::time::Duration;

#[test]
fn attempt_budgets_validate_inclusive_limits_and_round_trip() -> Result<(), Box<dyn Error>> {
    for attempts in [1, 3, 100] {
        let budget = MaxAttempts::try_from(attempts)?;
        assert_eq!(budget.get(), attempts);
        assert_eq!(u32::from(budget), attempts);
        assert_eq!(budget, MaxAttempts::new(attempts)?);
    }
    for attempts in [0, 101, u32::MAX] {
        assert_eq!(
            MaxAttempts::new(attempts),
            Err(RelayPolicyError::InvalidMaxAttempts)
        );
    }
    assert!(MaxAttempts::new(1)? < MaxAttempts::new(100)?);
    Ok(())
}

#[test]
fn backoff_validation_preserves_precision_and_rejects_invalid_intervals()
-> Result<(), Box<dyn Error>> {
    for (base, maximum) in [
        (RetryBackoff::MIN, RetryBackoff::MIN),
        (RetryBackoff::MAX, RetryBackoff::MAX),
        (
            Duration::from_nanos(1_999_999),
            Duration::from_nanos(9_999_999),
        ),
    ] {
        let backoff = RetryBackoff::new(base, maximum)?;
        assert_eq!(backoff.base_delay(), base);
        assert_eq!(backoff.max_delay(), maximum);
        for lease in [LeaseDuration::MIN, LeaseDuration::MAX] {
            assert!(
                RelayPolicy::new(
                    "relay_01",
                    LeaseDuration::new(lease)?,
                    MaxAttempts::new(100)?,
                    backoff
                )
                .is_ok()
            );
        }
    }
    for (base, maximum) in [
        (Duration::ZERO, RetryBackoff::MAX),
        (
            RetryBackoff::MIN - Duration::from_nanos(1),
            RetryBackoff::MAX,
        ),
        (Duration::from_secs(2), Duration::from_secs(1)),
        (
            RetryBackoff::MIN,
            RetryBackoff::MAX + Duration::from_nanos(1),
        ),
        (Duration::MAX, Duration::MAX),
    ] {
        assert_eq!(
            RetryBackoff::new(base, maximum),
            Err(RelayPolicyError::InvalidRetryBackoff)
        );
    }
    Ok(())
}

#[test]
fn owner_errors_are_startup_only_and_payload_safe() -> Result<(), Box<dyn Error>> {
    let backoff = RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(2))?;
    let budget = MaxAttempts::new(3)?;
    let lease = LeaseDuration::new(Duration::from_secs(30))?;
    for owner in [
        String::new(),
        "x".repeat(129),
        "Private-Owner-183".into(),
        "rélay".into(),
        "relay\nsecret".into(),
    ] {
        let error = RelayPolicy::new(&owner, lease, budget, backoff)
            .err()
            .ok_or("invalid owner accepted")?;
        assert_eq!(error, RelayPolicyError::InvalidLeaseOwner);
        assert!(error.source().is_none());
        if !owner.is_empty() {
            assert!(!format!("{error} {error:?}").contains(&owner));
        }
    }
    for owner in ["a".to_owned(), "x".repeat(128), "relay_01-valid".into()] {
        assert!(RelayPolicy::new(owner, lease, budget, backoff).is_ok());
    }
    Ok(())
}

#[test]
fn storage_failures_preserve_all_runtime_categories_and_real_source_presence()
-> Result<(), Box<dyn Error>> {
    for (storage, expected) in [
        (
            OutboxStoreErrorKind::Unavailable,
            RelayErrorKind::Unavailable,
        ),
        (
            OutboxStoreErrorKind::StateTransition,
            RelayErrorKind::StateTransition,
        ),
        (OutboxStoreErrorKind::Invariant, RelayErrorKind::Invariant),
    ] {
        for with_source in [false, true] {
            let store_error = if with_source {
                OutboxStoreError::with_source(
                    storage,
                    std::io::Error::other("private-storage-source-183"),
                )
            } else {
                OutboxStoreError::new(storage)
            };
            let error = RelayError::from(store_error);
            assert_eq!(error.kind(), expected);
            assert!(error.triggering_failure().is_none());
            assert!(!format!("{error} {error:?}").contains("private-storage-source-183"));
            let source = error
                .source()
                .and_then(|cause| cause.downcast_ref::<OutboxStoreError>())
                .ok_or("primary source missing")?;
            assert_eq!(source.kind(), storage);
            assert_eq!(source.source().is_some(), with_source);
        }
    }
    Ok(())
}

#[test]
fn configuration_and_runtime_types_preserve_worker_auto_traits() {
    fn send_sync<T: Send + Sync>() {}
    fn copy_value<T: Copy>() {}
    send_sync::<RelayPolicy>();
    send_sync::<RelayPolicyError>();
    send_sync::<RelayError>();
    send_sync::<MaxAttempts>();
    send_sync::<RetryBackoff>();
    copy_value::<MaxAttempts>();
    copy_value::<RetryBackoff>();
    copy_value::<RelayPolicyError>();
}
