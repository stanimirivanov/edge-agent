use super::*;

#[tokio::test]
async fn publication_budget_is_checked_before_publisher_invocation() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    // Includes reducing a previously larger budget, and overflow-sized history.
    for (attempt, limit) in [(1, 3), (3, 3), (4, 3), (3, 1), (u32::MAX, 100)] {
        let policy = RelayPolicy::new(
            "relay_port_test",
            LeaseDuration::new(Duration::from_secs(30))?,
            MaxAttempts::new(limit)?,
            RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?,
        )?;
        let template = claimed_message()?;
        let original = ClaimedMessage::new(
            template.message_source().to_owned(),
            template.message_id().to_owned(),
            template.message_type().to_owned(),
            template.transport_subject().to_owned(),
            template.envelope_bytes().to_vec(),
            attempt,
            template.lease_generation(),
        )?;
        let publisher = RecordingPublisher::default();
        let mut store = InMemoryStore {
            claimed: Some(original.clone()),
            ..Default::default()
        };
        let outcome = relay_once(&mut store, &registry, &publisher, &policy).await?;
        assert_eq!(
            publisher.0.load(Ordering::Relaxed),
            usize::from(attempt <= limit)
        );
        if attempt <= limit {
            assert!(
                matches!(outcome, RelayOutcome::Published { attempt: actual, .. } if actual == attempt)
            );
        } else {
            assert!(matches!(outcome, RelayOutcome::Quarantined {
                reason: QuarantineReason::PublicationBudgetExhausted,
                failure: RelayMessageFailure::PublicationBudgetExhausted,
                attempt: actual,
            } if actual == attempt));
            assert_eq!(
                store.quarantine_request,
                Some((original, "relay_port_test".into()))
            );
            assert_eq!(
                store.transitions,
                [
                    Transition::Claimed(LeaseDuration::new(Duration::from_secs(30))?),
                    Transition::Quarantined(CLAIM_GENERATION, "publication_budget_exhausted"),
                ]
            );
        }
    }
    Ok(())
}
