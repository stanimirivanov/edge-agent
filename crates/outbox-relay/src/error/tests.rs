use super::*;
use edgeagent_messaging::{OutboxRetryDelay, OutboxTimingError, PublishError, PublishErrorKind};
use std::time::Duration;

#[derive(Debug)]
struct PrivateCause(&'static str);
impl Display for PrivateCause {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl Error for PrivateCause {}

#[test]
fn runtime_errors_retain_primary_and_secondary_causes_without_rendering_them()
-> Result<(), Box<dyn Error>> {
    const PRIMARY: &str = "private-storage-source-183";
    const TRIGGER: &str = "private-publisher-source-183";
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
        let error = RelayError::from(OutboxStoreError::with_source(
            storage,
            PrivateCause(PRIMARY),
        ))
        .with_triggering_failure(RelayMessageFailure::Publication(
            PublishError::with_source(PublishErrorKind::ConfirmationUnknown, PrivateCause(TRIGGER)),
        ));
        assert_eq!(error.kind(), expected);
        let primary = error
            .source()
            .and_then(|cause| cause.downcast_ref::<OutboxStoreError>())
            .ok_or("storage source missing")?;
        assert_eq!(primary.kind(), storage);
        assert_eq!(
            primary
                .source()
                .and_then(|cause| cause.downcast_ref::<PrivateCause>())
                .map(|cause| cause.0),
            Some(PRIMARY)
        );
        let trigger = error
            .triggering_failure()
            .and_then(Error::source)
            .and_then(|cause| cause.downcast_ref::<PublishError>())
            .ok_or("publication trigger missing")?;
        assert_eq!(trigger.kind(), PublishErrorKind::ConfirmationUnknown);
        assert_eq!(
            trigger
                .source()
                .and_then(|cause| cause.downcast_ref::<PrivateCause>())
                .map(|cause| cause.0),
            Some(TRIGGER)
        );
        for secret in [PRIMARY, TRIGGER] {
            assert!(!format!("{error} {error:?}").contains(secret));
        }
    }
    Ok(())
}

#[test]
fn calculation_failures_are_invariants_with_actual_causes() -> Result<(), Box<dyn Error>> {
    let conversion = u64::try_from(u128::MAX)
        .err()
        .ok_or("expected real conversion failure")?;
    let error = RelayError::calculation_invariant(conversion);
    assert_eq!(error.kind(), RelayErrorKind::Invariant);
    assert!(
        error
            .source()
            .is_some_and(|cause| cause.is::<std::num::TryFromIntError>())
    );
    assert!(error.triggering_failure().is_none());
    let timing = OutboxRetryDelay::new(OutboxRetryDelay::MAX + Duration::from_nanos(1))
        .err()
        .ok_or("expected real timing failure")?;
    let error = RelayError::calculation_invariant(timing).with_triggering_failure(
        RelayMessageFailure::Publication(PublishError::new(PublishErrorKind::Unavailable)),
    );
    assert_eq!(error.kind(), RelayErrorKind::Invariant);
    assert!(
        error
            .source()
            .is_some_and(|cause| cause.is::<OutboxTimingError>())
    );
    let trigger = error
        .triggering_failure()
        .and_then(Error::source)
        .ok_or("trigger missing")?;
    assert!(trigger.is::<PublishError>());
    assert!(
        trigger.source().is_none(),
        "do not fabricate source-free publication causes"
    );
    Ok(())
}

#[test]
fn returned_outcomes_redact_nested_publication_causes() {
    const PRIVATE: &str = "private-relay-source-sentinel-7391";
    let outcome = crate::RelayOutcome::RetryScheduled {
        failure: PublishError::with_source(PublishErrorKind::Unavailable, PrivateCause(PRIVATE)),
        attempt: 1,
        delay: Duration::from_millis(1),
    };
    assert!(!format!("{outcome:?}").contains(PRIVATE));
    let failure = RelayMessageFailure::Publication(PublishError::with_source(
        PublishErrorKind::Unavailable,
        PrivateCause(PRIVATE),
    ));
    assert!(!format!("{failure} {failure:?}").contains(PRIVATE));
}
