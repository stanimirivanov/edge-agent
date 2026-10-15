use super::*;
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug)]
enum Transition {
    Enqueue,
    Claim,
    Publish,
    Retry,
    Quarantine,
    Replay,
}

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
struct State {
    attempt_count: i32,
    lease_generation: i64,
    lease_owner: Option<String>,
    available_at: String,
    lease_expires_at: Option<String>,
    published: bool,
    quarantined: bool,
    last_failure_code: Option<String>,
}

async fn state(connection: &mut PgConnection) -> TestResult<(Vec<State>, i64)> {
    let rows = sqlx::query_as(
        "SELECT attempt_count, lease_generation, lease_owner,
                available_at::text, lease_expires_at::text,
                published_at IS NOT NULL AS published,
                quarantined_at IS NOT NULL AS quarantined, last_failure_code
         FROM edgeagent_message_outbox",
    )
    .fetch_all(&mut *connection)
    .await?;
    let audit = sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox_replay_audit")
        .fetch_one(connection)
        .await?;
    Ok((rows, audit))
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn precommit_cancellation_rolls_back_each_transition_before_reuse() -> TestResult {
    for transition in [
        Transition::Enqueue,
        Transition::Claim,
        Transition::Publish,
        Transition::Retry,
        Transition::Quarantine,
        Transition::Replay,
    ] {
        with_fixture(move |fixture| precommit_case(fixture, transition)).await?;
    }
    Ok(())
}

async fn precommit_case(fixture: Fixture, transition: Transition) -> TestResult {
    let mut worker = fixture.connect().await?;
    let mut observer = fixture.connect().await?;
    let mut claimed = None;
    if !matches!(transition, Transition::Enqueue) {
        seed(&mut worker, "precommit").await?;
    }
    if !matches!(transition, Transition::Enqueue | Transition::Claim) {
        claimed = Some(claim(&mut worker).await?);
    }
    if matches!(transition, Transition::Replay) {
        let claim = claimed.as_ref().ok_or("missing replay claim")?;
        let mut transaction = worker.begin().await?;
        PostgresOutbox
            .quarantine(&mut transaction, claim, OWNER, FAILURE)
            .await?;
        transaction.commit().await?;
    }
    let before = state(&mut observer).await?;
    let entered = Arc::new(AtomicBool::new(false));
    let signal = entered.clone();
    let mut operation = Box::pin(async {
        // The future owns the transaction. Dropping this future queues
        // rollback; only later connection reuse proves it was processed.
        let mut transaction = worker.begin().await?;
        match transition {
            Transition::Enqueue => {
                PostgresOutbox
                    .enqueue(&mut transaction, COMMAND, &envelope("precommit", 1)?)
                    .await?;
            }
            Transition::Claim => {
                PostgresOutbox
                    .claim_batch(
                        &mut transaction,
                        OWNER,
                        1,
                        LeaseDuration::new(LeaseDuration::MAX)?,
                    )
                    .await?;
            }
            Transition::Replay => {
                PostgresOutbox
                    .replay_quarantined(
                        &mut transaction,
                        SOURCE,
                        "precommit",
                        replay_request("cancelled-replay")?,
                    )
                    .await?;
            }
            outcome => {
                let claim = claimed.as_ref().ok_or("missing outcome claim")?;
                match outcome {
                    Transition::Publish => {
                        PostgresOutbox
                            .mark_published(&mut transaction, claim, OWNER)
                            .await?;
                    }
                    Transition::Retry => {
                        PostgresOutbox
                            .release_for_retry(
                                &mut transaction,
                                claim,
                                OWNER,
                                OutboxRetryDelay::IMMEDIATE,
                                FAILURE,
                            )
                            .await?;
                    }
                    Transition::Quarantine => {
                        PostgresOutbox
                            .quarantine(&mut transaction, claim, OWNER, FAILURE)
                            .await?;
                    }
                    _ => return Err("unexpected outcome transition".into()),
                }
            }
        }
        signal.store(true, Ordering::Release);
        pending::<()>().await;
        transaction.commit().await?;
        Ok(()) as TestResult
    });
    tokio::select! {
        result = &mut operation => return Err(format!("operation ended before controlled cancellation: {result:?}").into()),
        () = async { while !entered.load(Ordering::Acquire) { tokio::task::yield_now().await; } } => {},
    }
    assert_eq!(
        state(&mut observer).await?,
        before,
        "uncommitted {transition:?} became visible"
    );
    drop(operation);
    let reusable: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&mut worker)
        .await?;
    assert_eq!(reusable, 1);
    assert_eq!(
        state(&mut observer).await?,
        before,
        "cancelled {transition:?} persisted after rollback/reuse"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn cancelled_blocked_outcome_recovers_with_a_new_fence() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut locker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        seed(&mut worker, "blocked-outcome").await?;
        let stale = claim(&mut worker).await?;
        let before = state(&mut observer).await?;
        let waiter = backend_pid(&mut worker).await?;
        let blocker = backend_pid(&mut locker).await?;
        let mut lock = locker.begin().await?;
        sqlx::query(
            "SELECT message_id FROM edgeagent_message_outbox
             WHERE message_id = 'blocked-outcome' FOR UPDATE",
        ).fetch_one(&mut *lock).await?;
        {
            let mut store = PostgresOutboxRelay::new(&mut worker);
            let mut operation = store.mark_published(&stale, OWNER);
            tokio::select! {
                result = &mut operation => return Err(format!("outcome ended before lock release: {result:?}").into()),
                blocked = observe_blocked(&mut observer, waiter, blocker) => blocked?,
            }
            // Exercise the actual relay future owning its top-level transaction.
            drop(operation);
        }
        // Query-future drop is not server-side cancellation. Release the blocker
        // before draining SQLx's pending response and queued rollback on reuse.
        lock.rollback().await?;
        let reusable: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&mut worker).await?;
        assert_eq!(reusable, 1);
        assert_eq!(state(&mut observer).await?, before);
        sqlx::query(
            "UPDATE edgeagent_message_outbox
             SET lease_expires_at = clock_timestamp() - INTERVAL '1 second'
             WHERE message_id = 'blocked-outcome'",
        ).execute(&mut observer).await?;
        let current = claim(&mut worker).await?;
        assert!(current.lease_generation().get() > stale.lease_generation().get());
        assert_eq!(current.attempt(), 2);
        let mut transaction = worker.begin().await?;
        let rejected = PostgresOutbox.mark_published(&mut transaction, &stale, OWNER).await;
        assert_eq!(rejected.err().map(|error| error.kind()), Some(OutboxErrorKind::LeaseLost));
        PostgresOutbox.mark_published(&mut transaction, &current, OWNER).await?;
        transaction.commit().await?;
        let (rows, audit) = state(&mut observer).await?;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].published);
        assert_eq!(u64::try_from(rows[0].lease_generation)?, current.lease_generation().get());
        assert_eq!(audit, 0);
        Ok(())
    }).await
}
