use super::*;

#[test]
fn typed_backoff_preserves_exact_legacy_jitter_and_millisecond_flooring()
-> Result<(), Box<dyn Error>> {
    for fraction in [Duration::ZERO, Duration::from_nanos(999_999)] {
        let policy = RelayPolicy::new(
            "relay_01",
            LeaseDuration::new(LeaseDuration::MAX)?,
            MaxAttempts::new(100)?,
            RetryBackoff::new(
                Duration::from_secs(2) + fraction,
                Duration::from_secs(5) + fraction,
            )?,
        )?;
        // Frozen pre-migration FNV/equal-jitter schedules, including capped and
        // overflow-sized history. Do not derive expectations using the subject.
        for (attempt, expected_millis) in [
            (1, 1560),
            (2, 3033),
            (3, 3850),
            (64, 3073),
            (u32::MAX, 3251),
        ] {
            let delay: OutboxRetryDelay =
                retry_delay(&policy, "urn:edgeagent:gateway", "message-01", attempt)?;
            assert_eq!(delay.get().as_millis(), expected_millis);
        }
    }
    Ok(())
}
