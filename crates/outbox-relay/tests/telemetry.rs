//! One isolated executable owns tracing callsites for the relay trust boundary.

#[path = "telemetry/support.rs"]
mod support;

use edgeagent_messaging::LeaseDuration;
use edgeagent_outbox_relay::{MaxAttempts, RetryBackoff};
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use edgeagent_contracts::MessageRegistry;
use edgeagent_messaging::{
    FailureCode, OutboxStoreError, OutboxStoreErrorKind, PublishDisposition, PublishError,
    PublishErrorKind,
};
use edgeagent_outbox_relay::{
    QuarantineReason, RelayError, RelayErrorKind, RelayMessageFailure, RelayOutcome, RelayPolicy,
    relay_once,
};
use support::{
    Calls, Capture, DEFINITION, Fields, OWNER, Operation, PRIVATE, Publisher, Store, claim,
};

fn assert_events(events: &[Fields], expected: &[(&str, &str)], trusted: bool, attempt: u32) {
    assert_eq!(events.len(), expected.len(), "{events:?}");
    for (fields, (stage, outcome)) in events.iter().zip(expected) {
        assert_eq!(
            fields.0.get("event_stage").map(String::as_str),
            Some(*stage)
        );
        assert_eq!(
            fields.0.get("event_outcome").map(String::as_str),
            Some(*outcome)
        );
        assert_eq!(fields.0.get("delivery_attempt"), Some(&attempt.to_string()));
        assert!(fields.0.contains_key("duration_seconds"));
        for field in [
            "message_id",
            "message_source",
            "message_type",
            "correlation_id",
            "causation_id",
            "trace_parent",
        ] {
            assert_eq!(
                fields.0.get(field).is_some_and(|value| !value.is_empty()),
                trusted,
                "{fields:?}"
            );
        }
        assert_eq!(
            fields.0.get("transport_message_key").map(String::as_str),
            Some("")
        );
        if trusted {
            for (field, expected) in [
                ("message_id", "message-01"),
                ("message_source", "urn:edgeagent:component:gateway"),
                ("message_type", DEFINITION.message_type()),
                ("correlation_id", "correlation-01"),
                ("causation_id", "command-01"),
                (
                    "trace_parent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                ),
            ] {
                assert_eq!(fields.0.get(field).map(String::as_str), Some(expected));
            }
        }
    }
    let debug = format!("{events:?}");
    assert!(!debug.contains(PRIVATE));
    assert!(!debug.contains(&format!("{:?}", PRIVATE.as_bytes())));
    assert!(!debug.contains("private_payload"));
}

fn assert_send<T: Send>(value: T) -> T {
    value
}

fn assert_runtime_failure(
    error: &RelayError,
    kind: OutboxStoreErrorKind,
) -> Result<(), Box<dyn Error>> {
    let expected = match kind {
        OutboxStoreErrorKind::Unavailable => RelayErrorKind::Unavailable,
        OutboxStoreErrorKind::StateTransition => RelayErrorKind::StateTransition,
        OutboxStoreErrorKind::Invariant => RelayErrorKind::Invariant,
    };
    assert_eq!(error.kind(), expected);
    let primary = error
        .source()
        .and_then(|cause| cause.downcast_ref::<OutboxStoreError>())
        .ok_or("primary storage source missing")?;
    assert_eq!(primary.kind(), kind);
    assert_eq!(
        primary
            .source()
            .and_then(|cause| cause.downcast_ref::<std::io::Error>())
            .map(ToString::to_string)
            .as_deref(),
        Some(PRIVATE)
    );
    assert!(!format!("{error} {error:?}").contains(PRIVATE));
    Ok(())
}

/// A single test avoids competing subscribers and process-global interest races.
#[tokio::test(flavor = "current_thread")]
async fn relay_observations_respect_validation_and_confirmed_outcomes() -> Result<(), Box<dyn Error>>
{
    let events = Arc::new(Mutex::new(Vec::new()));
    let _subscriber = tracing::subscriber::set_default(Capture(Arc::clone(&events)));
    let definitions = [DEFINITION];
    let registry = MessageRegistry::new(&definitions)?;
    let empty_registry = MessageRegistry::new(&[])?;
    let policy = RelayPolicy::new(
        OWNER,
        LeaseDuration::new(Duration::from_secs(30))?,
        MaxAttempts::new(3)?,
        RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?,
    )?;

    // Include mismatched columns, malformed bytes, and a valid-but-unsupported type.
    for rejected in ["source", "id", "type", "bytes", "unsupported"] {
        for fault in [
            None,
            Some((Operation::Quarantine, OutboxStoreErrorKind::Unavailable)),
            Some((Operation::Quarantine, OutboxStoreErrorKind::StateTransition)),
            Some((Operation::Quarantine, OutboxStoreErrorKind::Invariant)),
        ] {
            let original = claim(2, Some(rejected))?;
            let calls = Calls::default();
            let mut store = Store {
                claim: Some(original.clone()),
                original: original.clone(),
                calls: Arc::clone(&calls),
                fault,
                code: None,
            };
            let publisher = Publisher {
                result: Ok(PublishDisposition::Persisted),
                calls: Arc::clone(&calls),
                original,
            };
            let selected_registry = if rejected == "unsupported" {
                &empty_registry
            } else {
                &registry
            };
            let result = assert_send(relay_once(
                &mut store,
                selected_registry,
                &publisher,
                &policy,
            ))
            .await;
            if let Some((_, kind)) = fault {
                let error = result
                    .err()
                    .ok_or("uncommitted contract quarantine must fail")?;
                assert_runtime_failure(&error, kind)?;
                assert!(matches!(
                    error.triggering_failure(),
                    Some(RelayMessageFailure::StoredContract(_))
                ));
            } else {
                assert!(matches!(
                    result?,
                    RelayOutcome::Quarantined {
                        reason: QuarantineReason::StoredContractInvalid,
                        ..
                    }
                ));
            }
            assert_eq!(
                *calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                [Operation::Claim, Operation::Quarantine]
            );
            let captured = std::mem::take(
                &mut *events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            assert_events(
                &captured,
                &[(
                    "persistence",
                    if fault.is_some() {
                        "failed"
                    } else {
                        "quarantined"
                    },
                )],
                false,
                2,
            );
        }
    }

    for (publication, attempt, operation, publication_outcome, persistence_outcome, code) in [
        (
            Ok(PublishDisposition::Persisted),
            1,
            Operation::Mark,
            "succeeded",
            "succeeded",
            None,
        ),
        (
            Ok(PublishDisposition::Duplicate),
            1,
            Operation::Mark,
            "duplicate",
            "succeeded",
            None,
        ),
        (
            Err(PublishErrorKind::Unavailable),
            1,
            Operation::Retry,
            "failed",
            "retry_scheduled",
            Some("transport_unavailable"),
        ),
        (
            Err(PublishErrorKind::ConfirmationUnknown),
            1,
            Operation::Retry,
            "failed",
            "retry_scheduled",
            Some("confirmation_unknown"),
        ),
        (
            Err(PublishErrorKind::Unavailable),
            3,
            Operation::Quarantine,
            "failed",
            "quarantined",
            Some("attempts_exhausted_unavailable"),
        ),
        (
            Err(PublishErrorKind::ConfirmationUnknown),
            3,
            Operation::Quarantine,
            "failed",
            "quarantined",
            Some("attempts_exhausted_confirmation_unknown"),
        ),
        (
            Err(PublishErrorKind::Contract),
            1,
            Operation::Quarantine,
            "failed",
            "quarantined",
            Some("publish_contract"),
        ),
        (
            Err(PublishErrorKind::Rejected),
            1,
            Operation::Quarantine,
            "failed",
            "quarantined",
            Some("transport_rejected"),
        ),
    ] {
        for fault in [
            None,
            Some((operation, OutboxStoreErrorKind::Unavailable)),
            Some((operation, OutboxStoreErrorKind::StateTransition)),
            Some((operation, OutboxStoreErrorKind::Invariant)),
            Some((Operation::Claim, OutboxStoreErrorKind::Unavailable)),
            Some((Operation::Claim, OutboxStoreErrorKind::StateTransition)),
            Some((Operation::Claim, OutboxStoreErrorKind::Invariant)),
        ] {
            let original = claim(attempt, None)?;
            let calls = Calls::default();
            let mut store = Store {
                claim: Some(original.clone()),
                original: original.clone(),
                calls: Arc::clone(&calls),
                fault,
                code: None,
            };
            let publisher = Publisher {
                result: publication,
                calls: Arc::clone(&calls),
                original,
            };
            let result = assert_send(relay_once(&mut store, &registry, &publisher, &policy)).await;
            if let Some((failed_operation, kind)) = fault {
                let error = result
                    .err()
                    .ok_or("storage failure must fail the iteration")?;
                let source = error
                    .source()
                    .and_then(|source| source.downcast_ref::<OutboxStoreError>())
                    .ok_or("storage cause missing")?;
                assert_eq!(source.kind(), kind);
                assert_eq!(
                    source
                        .source()
                        .and_then(|source| source.downcast_ref::<std::io::Error>())
                        .map(ToString::to_string)
                        .as_deref(),
                    Some(PRIVATE)
                );
                assert!(!format!("{error} {error:?}").contains(PRIVATE));
                assert_runtime_failure(&error, kind)?;
                if failed_operation == Operation::Claim || publication.is_ok() {
                    assert!(error.triggering_failure().is_none());
                } else {
                    let trigger = error
                        .triggering_failure()
                        .and_then(Error::source)
                        .and_then(|cause| cause.downcast_ref::<PublishError>())
                        .ok_or("publication trigger missing")?;
                    assert_eq!(Err(trigger.kind()), publication);
                    assert_eq!(
                        trigger
                            .source()
                            .and_then(|cause| cause.downcast_ref::<std::io::Error>())
                            .map(ToString::to_string)
                            .as_deref(),
                        Some(PRIVATE)
                    );
                }
            } else {
                match result? {
                    RelayOutcome::Published {
                        disposition,
                        attempt: actual,
                    } => {
                        assert_eq!(Ok(disposition), publication);
                        assert_eq!(actual, attempt);
                    }
                    RelayOutcome::RetryScheduled {
                        failure,
                        attempt: actual,
                        ..
                    } => {
                        assert_eq!(Err(failure.kind()), publication);
                        assert_eq!(actual, attempt);
                        assert!(failure.source().is_some());
                    }
                    RelayOutcome::Quarantined {
                        reason,
                        attempt: actual,
                        failure,
                    } => {
                        assert_eq!(Some(reason.code().as_str()), code);
                        assert_eq!(actual, attempt);
                        assert!(
                            failure
                                .source()
                                .and_then(|cause| cause.downcast_ref::<PublishError>())
                                .and_then(Error::source)
                                .is_some()
                        );
                    }
                    RelayOutcome::Idle => return Err("unexpected idle result".into()),
                }
            }
            let claim_failed = fault.is_some_and(|(op, _)| op == Operation::Claim);
            assert_eq!(
                *calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                if claim_failed {
                    vec![Operation::Claim]
                } else {
                    vec![Operation::Claim, Operation::Publish, operation]
                }
            );
            assert_eq!(
                store.code.map(FailureCode::as_str),
                if claim_failed { None } else { code }
            );
            let captured = std::mem::take(
                &mut *events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            let expected = if claim_failed {
                vec![]
            } else {
                vec![
                    ("publication", publication_outcome),
                    (
                        "persistence",
                        if fault.is_some() {
                            "failed"
                        } else {
                            persistence_outcome
                        },
                    ),
                ]
            };
            assert_events(&captured, &expected, true, attempt);
        }
    }
    // Exhaustion has no publication event: no publisher operation occurred.
    // Invalid records still take the identity-free contract path when over budget.
    for rejected in [None, Some("bytes"), Some("unsupported")] {
        for fault in [
            None,
            Some(OutboxStoreErrorKind::Unavailable),
            Some(OutboxStoreErrorKind::StateTransition),
            Some(OutboxStoreErrorKind::Invariant),
        ] {
            let original = claim(4, rejected)?;
            let calls = Calls::default();
            let mut store = Store {
                claim: Some(original.clone()),
                original: original.clone(),
                calls: Arc::clone(&calls),
                fault: fault.map(|kind| (Operation::Quarantine, kind)),
                code: None,
            };
            let publisher = Publisher {
                result: Ok(PublishDisposition::Persisted),
                calls: Arc::clone(&calls),
                original,
            };
            let selected_registry = if rejected == Some("unsupported") {
                &empty_registry
            } else {
                &registry
            };
            let result = relay_once(&mut store, selected_registry, &publisher, &policy).await;
            if let Some(kind) = fault {
                let error = result.err().ok_or("uncommitted exhaustion must fail")?;
                assert_runtime_failure(&error, kind)?;
                if rejected.is_some() {
                    assert!(matches!(
                        error.triggering_failure(),
                        Some(RelayMessageFailure::StoredContract(_))
                    ));
                } else {
                    assert!(matches!(
                        error.triggering_failure(),
                        Some(RelayMessageFailure::PublicationBudgetExhausted)
                    ));
                    assert!(error.triggering_failure().and_then(Error::source).is_none());
                }
                assert_eq!(
                    error
                        .source()
                        .and_then(|cause| cause.downcast_ref::<OutboxStoreError>())
                        .map(OutboxStoreError::kind),
                    Some(kind)
                );
            } else if rejected.is_none() {
                let RelayOutcome::Quarantined {
                    reason,
                    failure,
                    attempt: 4,
                } = result?
                else {
                    return Err("expected policy exhaustion".into());
                };
                assert_eq!(reason, QuarantineReason::PublicationBudgetExhausted);
                assert!(matches!(
                    failure,
                    RelayMessageFailure::PublicationBudgetExhausted
                ));
                assert!(failure.source().is_none());
                assert_eq!(
                    failure.to_string(),
                    "relay publication claim budget is exhausted"
                );
            } else {
                assert!(matches!(
                    result?,
                    RelayOutcome::Quarantined {
                        reason: QuarantineReason::StoredContractInvalid,
                        ..
                    }
                ));
            }
            assert_eq!(
                *calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                [Operation::Claim, Operation::Quarantine]
            );
            assert_eq!(
                store.code.map(FailureCode::as_str),
                Some(if rejected.is_some() {
                    "stored_contract_invalid"
                } else {
                    "publication_budget_exhausted"
                })
            );
            let captured = std::mem::take(
                &mut *events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            assert_events(
                &captured,
                &[(
                    "persistence",
                    if fault.is_some() {
                        "failed"
                    } else {
                        "quarantined"
                    },
                )],
                rejected.is_none(),
                4,
            );
        }
    }
    Ok(())
}
