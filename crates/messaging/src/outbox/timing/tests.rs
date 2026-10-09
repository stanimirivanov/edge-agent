use super::{LeaseDuration, OutboxRetryDelay, OutboxTimingError};
use std::collections::HashSet;
use std::time::Duration;

#[test]
fn lease_duration_accepts_inclusive_bounds_and_preserves_precision() -> Result<(), OutboxTimingError>
{
    for valid in [
        LeaseDuration::MIN,
        Duration::from_micros(1_500),
        LeaseDuration::MAX,
    ] {
        let duration = LeaseDuration::new(valid)?;
        assert_eq!(duration.get(), valid);
        assert_eq!(Duration::from(duration), valid);
        assert_eq!(LeaseDuration::try_from(valid)?, duration);
    }
    for invalid in [
        Duration::ZERO,
        LeaseDuration::MIN - Duration::from_nanos(1),
        LeaseDuration::MAX + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        assert_eq!(
            LeaseDuration::new(invalid),
            Err(OutboxTimingError::InvalidLeaseDuration)
        );
        assert_eq!(
            LeaseDuration::try_from(invalid),
            Err(OutboxTimingError::InvalidLeaseDuration)
        );
    }
    Ok(())
}

#[test]
fn retry_delay_accepts_immediate_and_submillisecond_values() -> Result<(), OutboxTimingError> {
    assert_eq!(OutboxRetryDelay::IMMEDIATE.get(), Duration::ZERO);
    for valid in [
        Duration::ZERO,
        Duration::from_nanos(1),
        OutboxRetryDelay::MAX,
    ] {
        let delay = OutboxRetryDelay::new(valid)?;
        assert_eq!(delay.get(), valid);
        assert_eq!(Duration::from(delay), valid);
        assert_eq!(OutboxRetryDelay::try_from(valid)?, delay);
    }
    for invalid in [
        OutboxRetryDelay::MAX + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        assert_eq!(
            OutboxRetryDelay::new(invalid),
            Err(OutboxTimingError::InvalidRetryDelay)
        );
        assert_eq!(
            OutboxRetryDelay::try_from(invalid),
            Err(OutboxTimingError::InvalidRetryDelay)
        );
    }
    Ok(())
}

#[test]
fn timing_values_support_ordering_and_hashing() -> Result<(), OutboxTimingError> {
    let minimum_lease = LeaseDuration::new(LeaseDuration::MIN)?;
    let maximum_lease = LeaseDuration::new(LeaseDuration::MAX)?;
    assert!(minimum_lease < maximum_lease);
    assert_eq!(
        HashSet::from([minimum_lease, minimum_lease, maximum_lease]).len(),
        2
    );
    let maximum_retry = OutboxRetryDelay::new(OutboxRetryDelay::MAX)?;
    assert!(OutboxRetryDelay::IMMEDIATE < maximum_retry);
    assert_eq!(
        HashSet::from([OutboxRetryDelay::IMMEDIATE, maximum_retry, maximum_retry]).len(),
        2
    );
    Ok(())
}

#[test]
fn timing_values_and_errors_satisfy_port_auto_traits() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<LeaseDuration>();
    assert_send_sync_static::<OutboxRetryDelay>();
    assert_send_sync_static::<OutboxTimingError>();
}
