use super::*;
use edgeagent_messaging::LeaseDuration;
use edgeagent_messaging::{
    ClaimedMessage, LeaseGeneration, OutboxRelayStore, OutboxStoreErrorKind,
};
use edgeagent_outbox_relay::RelayMessageFailure;
use edgeagent_outbox_relay::{MaxAttempts, RetryBackoff};
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct PendingPublisher {
    calls: AtomicUsize,
    entered: Notify,
}

impl MessagePublisher for PendingPublisher {
    fn publish<'a>(&'a self, _: MessageDefinition, _: &'a MessageEnvelope) -> PublishFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.entered.notify_one();
            pending().await
        })
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn cancelled_publication_recovers_over_budget_without_another_publish() -> TestResult {
    with_schema(cancelled_publication).await
}

async fn cancelled_publication(fixture: Fixture) -> TestResult {
    let mut client = fixture.connect().await?;
    let mut inspector = fixture.connect().await?;
    for migration in PostgresOutbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }
    let command = envelope("relay-cancel-budget-01")?;
    enqueue(&mut client, command.id()).await?;
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "relay_test",
        LeaseDuration::new(Duration::from_secs(30))?,
        MaxAttempts::new(1)?,
        RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(2))?,
    )?;
    let publisher = PendingPublisher::default();
    {
        let mut store = PostgresOutboxRelay::new(&mut client);
        let work = relay_once(&mut store, &registry, &publisher, &policy);
        tokio::pin!(work);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                outcome = &mut work => Err(io::Error::other(format!("relay finished before cancellation: {outcome:?}"))),
                () = publisher.entered.notified() => Ok(()),
            }
        }).await??;
        // Drop the pending relay, releasing its connection borrow, not its lease.
    }
    let row = sqlx::query("SELECT lease_owner, lease_generation, attempt_count, envelope, published_at IS NULL AND quarantined_at IS NULL AS unresolved FROM edgeagent_message_outbox")
        .fetch_one(&mut inspector).await?;
    assert_eq!(row.try_get::<String, _>("lease_owner")?, "relay_test");
    assert_eq!(row.try_get::<i32, _>("attempt_count")?, 1);
    assert!(row.try_get::<bool, _>("unresolved")?);
    let generation = u64::try_from(row.try_get::<i64, _>("lease_generation")?)?;
    let bytes = row.try_get::<Vec<u8>, _>("envelope")?;
    assert_eq!(bytes, command.to_json()?);
    let stale = ClaimedMessage::new(
        command.source().into(),
        command.id().into(),
        command.message_type().into(),
        COMMAND.subject()?,
        bytes.clone(),
        1,
        LeaseGeneration::new(generation)?,
    )?;
    // Explicitly change only this owned schema's synthetic lease; no timing sleep.
    sqlx::query("UPDATE edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - interval '1 second'")
        .execute(&mut inspector).await?;
    let outcome = relay_once_with_postgres(&mut client, &registry, &publisher, &policy).await?;
    assert!(matches!(
        outcome,
        RelayOutcome::Quarantined {
            reason: QuarantineReason::PublicationBudgetExhausted,
            failure: RelayMessageFailure::PublicationBudgetExhausted,
            attempt: 2,
        }
    ));
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
    let row = sqlx::query("SELECT message_source, message_id, envelope, attempt_count, lease_generation, quarantine_reason, published_at IS NULL AND quarantined_at IS NOT NULL AND lease_owner IS NULL AS terminal FROM edgeagent_message_outbox")
        .fetch_one(&mut inspector).await?;
    assert_eq!(
        row.try_get::<String, _>("message_source")?,
        command.source()
    );
    assert_eq!(row.try_get::<String, _>("message_id")?, command.id());
    assert_eq!(row.try_get::<Vec<u8>, _>("envelope")?, bytes);
    assert_eq!(row.try_get::<i32, _>("attempt_count")?, 2);
    assert_eq!(
        u64::try_from(row.try_get::<i64, _>("lease_generation")?)?,
        generation + 1
    );
    assert_eq!(
        row.try_get::<String, _>("quarantine_reason")?,
        "publication_budget_exhausted"
    );
    assert!(row.try_get::<bool, _>("terminal")?);
    let mut store = PostgresOutboxRelay::new(&mut client);
    let error = store
        .mark_published(&stale, "relay_test")
        .await
        .err()
        .ok_or("stale completion succeeded")?;
    assert_eq!(error.kind(), OutboxStoreErrorKind::StateTransition);
    Ok(())
}
