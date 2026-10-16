//! Real broker persistence followed by application result loss, not wire ACK loss.

use super::*;
use edgeagent_messaging::LeaseDuration;
use edgeagent_messaging::{MessagePublisher, PublishFuture};
use edgeagent_outbox_relay::{MaxAttempts, RetryBackoff};
use edgeagent_outbox_relay::{QuarantineReason, RelayMessageFailure};
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[path = "../../../outbox-postgres/tests/support/schema.rs"]
mod schema;
use schema::{Fixture, TestResult, with_schema};

const DEFINITION: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.publication-recovery.v1",
    "urn:edgeagent:schema:publication-recovery:v1",
    Component::ExecutionSimulator,
    "order",
);

struct HoldConfirmedReceipt {
    inner: JetStreamPublisher,
    persisted: Notify,
    calls: AtomicUsize,
}

impl MessagePublisher for HoldConfirmedReceipt {
    fn publish<'a>(
        &'a self,
        definition: MessageDefinition,
        envelope: &'a MessageEnvelope,
    ) -> PublishFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let receipt = self.inner.publish(definition, envelope).await?;
            assert_eq!(receipt.disposition(), PublishDisposition::Persisted);
            // Only signal after production JetStream publication returned confirmation.
            self.persisted.notify_one();
            pending().await
        })
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL, EDGEAGENT_POSTGRES_URL, and isolated services"]
async fn producer_result_loss_recovers_within_budget_and_quarantines_after_exhaustion() -> TestResult
{
    // Each case owns its schema. Its outer deadline leaves room for bounded
    // connection/setup, inner cancellation, and broker cleanup before DB cleanup.
    for budget in [2, 1] {
        with_schema(move |fixture| verify_producer_recovery(fixture, budget)).await?;
    }
    Ok(())
}

async fn verify_producer_recovery(fixture: Fixture, budget: u32) -> TestResult {
    let client = tokio::time::timeout(
        Duration::from_secs(5),
        async_nats::connect(env::var("EDGEAGENT_NATS_URL")?),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "producer proof NATS connection timed out",
        )
    })??;
    let context = jetstream::ContextBuilder::new()
        .timeout(Duration::from_secs(5))
        .build(client);
    let name = format!("EA_PRODUCER_RECOVERY_{}_{}", process::id(), budget);
    let stream = context
        .create_stream(StreamConfig {
            name: name.clone(),
            subjects: vec![DEFINITION.subject()?],
            storage: StorageType::Memory,
            duplicate_window: Duration::from_secs(120),
            ..StreamConfig::default()
        })
        .await?;
    // An owned task catches errors/panics; abort and await before resource cleanup.
    let work_context = context.clone();
    let mut work =
        tokio::spawn(async move { producer_case(fixture, budget, work_context, stream).await });
    let result: TestResult = match tokio::time::timeout(Duration::from_secs(10), &mut work).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(error.into()),
        Err(error) => {
            work.abort();
            match tokio::time::timeout(Duration::from_secs(1), work).await {
                Ok(_) => Err(error.into()),
                Err(shutdown) => Err(format!(
                    "producer proof timed out ({error}); task shutdown timed out ({shutdown})"
                )
                .into()),
            }
        }
    };
    let cleanup: TestResult = async {
        if !context.delete_stream(name).await?.success {
            return Err("producer stream deletion was not confirmed".into());
        }
        Ok(())
    }
    .await;
    match (result, cleanup) {
        (Ok(()), cleanup) => cleanup,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("producer proof failed ({error}); cleanup failed ({cleanup})").into())
        }
    }
}

async fn producer_case(
    fixture: Fixture,
    budget: u32,
    context: jetstream::Context,
    mut stream: jetstream::stream::Stream,
) -> TestResult {
    let mut database = fixture.connect().await?;
    for migration in PostgresOutbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut database).await?;
    }
    let id = format!("producer-recovery-budget-{budget}");
    let command = DEFINITION.build(
        MessageMetadata {
            id: id.clone(),
            source: Component::Gateway.source_uri().into(),
            message_type: DEFINITION.message_type().into(),
            subject: format!("order/{id}"),
            time: "2026-10-16T00:00:00Z".into(),
            data_schema: DEFINITION.data_schema().into(),
            correlation_id: "producer-recovery-01".into(),
            causation_id: "producer-request-01".into(),
            idempotency_key: id.clone(),
            partition_key: format!("order/{id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        &json!({"mode": "dry_run", "quantity": 1}),
    )?;
    let mut transaction = database.begin().await?;
    PostgresOutbox
        .enqueue(&mut transaction, DEFINITION, &command)
        .await?;
    transaction.commit().await?;
    let definitions = [DEFINITION];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = RelayPolicy::new(
        "producer_recovery",
        LeaseDuration::new(Duration::from_secs(30))?,
        MaxAttempts::new(budget)?,
        RetryBackoff::new(Duration::from_millis(10), Duration::from_secs(1))?,
    )?;
    let held = HoldConfirmedReceipt {
        inner: JetStreamPublisher::from_context(context.clone()),
        persisted: Notify::new(),
        calls: AtomicUsize::new(0),
    };
    {
        let mut store = PostgresOutboxRelay::new(&mut database);
        let work = relay_once(&mut store, &registry, &held, &policy);
        tokio::pin!(work);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                outcome = &mut work => Err(io::Error::other(format!("relay unexpectedly returned {outcome:?}"))),
                () = held.persisted.notified() => Ok(()),
            }
        }).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut,
            "producer proof did not reach the held confirmed receipt"))??;
        // Cancel after confirmed broker persistence, before relay sees the receipt.
    }
    assert_eq!(held.calls.load(Ordering::SeqCst), 1);
    let row = sqlx::query("SELECT envelope, lease_generation, lease_owner, attempt_count, published_at IS NULL AND quarantined_at IS NULL AS unresolved FROM edgeagent_message_outbox WHERE message_id = $1")
        .bind(&id).fetch_one(&mut database).await?;
    assert!(row.try_get::<bool, _>("unresolved")?);
    assert_eq!(
        row.try_get::<String, _>("lease_owner")?,
        "producer_recovery"
    );
    assert_eq!(row.try_get::<i32, _>("attempt_count")?, 1);
    let generation: i64 = row.try_get("lease_generation")?;
    let original_bytes: Vec<u8> = row.try_get("envelope")?;
    assert_eq!(original_bytes, command.to_json()?);
    assert_eq!(stream.info().await?.state.messages, 1);
    let persisted = stream.get_raw_message(1).await?;
    assert_eq!(persisted.payload.as_ref(), original_bytes);
    sqlx::query("UPDATE edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - interval '1 second' WHERE message_id = $1")
        .bind(&id).execute(&mut database).await?;
    // A narrow counting decorator detects even invocation of an unpolled future.
    let recovered = CountingPublisher {
        inner: JetStreamPublisher::from_context(context),
        calls: AtomicUsize::new(0),
    };
    let mut store = PostgresOutboxRelay::new(&mut database);
    let outcome = relay_once(&mut store, &registry, &recovered, &policy).await?;
    if budget == 2 {
        assert!(matches!(
            outcome,
            RelayOutcome::Published {
                disposition: PublishDisposition::Duplicate,
                attempt: 2
            }
        ));
    } else {
        assert!(matches!(
            outcome,
            RelayOutcome::Quarantined {
                reason: QuarantineReason::PublicationBudgetExhausted,
                failure: RelayMessageFailure::PublicationBudgetExhausted,
                attempt: 2,
            }
        ));
    }
    assert_eq!(
        recovered.calls.load(Ordering::SeqCst),
        usize::from(budget == 2)
    );
    assert_eq!(stream.info().await?.state.messages, 1);
    let row = sqlx::query("SELECT message_source, message_id, envelope, attempt_count, lease_generation, lease_owner IS NULL AS released, published_at IS NOT NULL AS published, quarantine_reason FROM edgeagent_message_outbox WHERE message_id = $1")
        .bind(&id).fetch_one(&mut database).await?;
    assert_eq!(
        row.try_get::<String, _>("message_source")?,
        command.source()
    );
    assert_eq!(row.try_get::<String, _>("message_id")?, command.id());
    assert_eq!(row.try_get::<Vec<u8>, _>("envelope")?, original_bytes);
    assert_eq!(row.try_get::<i64, _>("lease_generation")?, generation + 1);
    assert_eq!(row.try_get::<i32, _>("attempt_count")?, 2);
    assert!(row.try_get::<bool, _>("released")?);
    assert_eq!(row.try_get::<bool, _>("published")?, budget == 2);
    assert_eq!(
        row.try_get::<Option<String>, _>("quarantine_reason")?
            .as_deref(),
        if budget == 1 {
            Some("publication_budget_exhausted")
        } else {
            None
        }
    );
    Ok(())
}

struct CountingPublisher {
    inner: JetStreamPublisher,
    calls: AtomicUsize,
}

impl MessagePublisher for CountingPublisher {
    fn publish<'a>(
        &'a self,
        definition: MessageDefinition,
        envelope: &'a MessageEnvelope,
    ) -> PublishFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.publish(definition, envelope)
    }
}
