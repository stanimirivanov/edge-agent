//! Opt-in proofs of top-level relay commits and classified database failures.

#![allow(
    clippy::disallowed_methods,
    reason = "Conformance fixtures administer isolated schemas and inject database failures."
)]

use edgeagent_contracts::{Component, MessageDefinition, MessageMetadata};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, OutboxRelayStore, OutboxRetryDelay,
    OutboxStoreError, OutboxStoreErrorKind,
};
use edgeagent_outbox_postgres::{OutboxError, PostgresOutbox, PostgresOutboxRelay};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use std::error::Error;
mod support;
use support::{TestResult, with_fixture};

const OWNER: &str = "relay_boundary";
const FAILURE: FailureCode = FailureCode::from_static("transport_unavailable");
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

async fn seed(connection: &mut PgConnection, message_id: &str) -> TestResult {
    let envelope = COMMAND.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: "order/relay-boundary".to_owned(),
            time: "2026-10-15T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: "relay-boundary-correlation".to_owned(),
            causation_id: "relay-boundary-request".to_owned(),
            idempotency_key: message_id.to_owned(),
            partition_key: "order/relay-boundary".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"quantity": 1}),
    )?;
    let mut transaction = connection.begin().await?;
    PostgresOutbox
        .enqueue(&mut transaction, COMMAND, &envelope)
        .await?;
    transaction.commit().await?;
    Ok(())
}

async fn claim(
    connection: &mut PgConnection,
) -> Result<ClaimedMessage, Box<dyn Error + Send + Sync>> {
    PostgresOutboxRelay::new(connection)
        .claim_one(OWNER, LeaseDuration::new(LeaseDuration::MAX)?)
        .await?
        .ok_or_else(|| "expected an available outbox message".into())
}

#[derive(Clone, Copy)]
enum Operation {
    Claim,
    Publish,
    Retry,
    Quarantine,
}

async fn apply(
    connection: &mut PgConnection,
    operation: Operation,
    claim: &ClaimedMessage,
) -> Result<(), OutboxStoreError> {
    let mut relay = PostgresOutboxRelay::new(connection);
    match operation {
        Operation::Claim => relay
            .claim_one(
                OWNER,
                LeaseDuration::new(LeaseDuration::MAX).map_err(|error| {
                    OutboxStoreError::with_source(OutboxStoreErrorKind::Invariant, error)
                })?,
            )
            .await
            .map(|_| ()),
        Operation::Publish => relay.mark_published(claim, OWNER).await,
        Operation::Retry => {
            relay
                .release_for_retry(claim, OWNER, OutboxRetryDelay::IMMEDIATE, FAILURE)
                .await
        }
        Operation::Quarantine => relay.quarantine(claim, OWNER, FAILURE).await,
    }
}

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
struct StoredState {
    attempt_count: i32,
    lease_generation: i64,
    lease_owner: Option<String>,
    published: bool,
    quarantined: bool,
    last_failure_code: Option<String>,
}

async fn state(
    connection: &mut PgConnection,
    message_id: &str,
) -> Result<StoredState, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT attempt_count, lease_generation, lease_owner,
               published_at IS NOT NULL AS published,
               quarantined_at IS NOT NULL AS quarantined,
               last_failure_code
        FROM edgeagent_message_outbox
        WHERE message_id = $1
        ",
    )
    .bind(message_id)
    .fetch_one(connection)
    .await
}

fn assert_invariant(error: &OutboxStoreError) {
    assert_eq!(error.kind(), OutboxStoreErrorKind::Invariant);
    assert_eq!(error.to_string(), "outbox storage invariant failed");
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn nested_relay_operations_preserve_the_callers_transaction() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        seed(&mut worker, "nested-message").await?;
        let claimed = claim(&mut worker).await?;
        let before = state(&mut observer, claimed.message_id()).await?;
        seed(&mut worker, "nested-available-message").await?;
        let available_before = state(&mut observer, "nested-available-message").await?;
        sqlx::raw_sql("CREATE TABLE application_data (id INTEGER PRIMARY KEY)")
            .execute(&mut worker)
            .await?;

        for (index, operation) in [
            Operation::Claim,
            Operation::Publish,
            Operation::Retry,
            Operation::Quarantine,
        ]
        .into_iter()
        .enumerate()
        {
            for commit in [false, true] {
                let id = i32::try_from(index)?;
                let mut outer = worker.begin().await?;
                sqlx::query("INSERT INTO application_data (id) VALUES ($1)")
                    .bind(id)
                    .execute(&mut *outer)
                    .await?;
                let error = apply(&mut outer, operation, &claimed)
                    .await
                    .err()
                    .ok_or("nested relay operation unexpectedly succeeded")?;
                assert_invariant(&error);
                assert!(error.source().is_none());
                let inside: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM application_data WHERE id = $1")
                        .bind(id)
                        .fetch_one(&mut *outer)
                        .await?;
                let outside: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM application_data WHERE id = $1")
                        .bind(id)
                        .fetch_one(&mut observer)
                        .await?;
                assert_eq!(inside, 1);
                assert_eq!(outside, 0);
                assert_eq!(state(&mut observer, claimed.message_id()).await?, before);
                assert_eq!(
                    state(&mut observer, "nested-available-message").await?,
                    available_before
                );
                if commit {
                    outer.commit().await?;
                } else {
                    outer.rollback().await?;
                }
                let committed: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM application_data WHERE id = $1")
                        .bind(id)
                        .fetch_one(&mut observer)
                        .await?;
                assert_eq!(committed, i64::from(commit));
            }
        }
        assert_eq!(state(&mut observer, claimed.message_id()).await?, before);
        assert_eq!(
            state(&mut observer, "nested-available-message").await?,
            available_before
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn successful_relay_operations_are_visible_after_return() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        for (operation, id) in [
            (Operation::Publish, "published-message"),
            (Operation::Retry, "retry-message"),
            (Operation::Quarantine, "quarantine-message"),
        ] {
            seed(&mut worker, id).await?;
            let claimed = claim(&mut worker).await?;
            assert_eq!(claimed.message_id(), id);
            let leased = state(&mut observer, id).await?;
            assert_eq!(leased.lease_owner.as_deref(), Some(OWNER));
            assert_eq!(leased.attempt_count, 1);
            assert_eq!(
                leased.lease_generation,
                i64::try_from(claimed.lease_generation().get())?
            );
            if matches!(operation, Operation::Retry) {
                // Keep the released record unavailable while testing the next message.
                PostgresOutboxRelay::new(&mut worker)
                    .release_for_retry(
                        &claimed,
                        OWNER,
                        OutboxRetryDelay::new(OutboxRetryDelay::MAX)?,
                        FAILURE,
                    )
                    .await?;
            } else {
                apply(&mut worker, operation, &claimed).await?;
            }
            let completed = state(&mut observer, id).await?;
            assert_eq!(completed.lease_owner, None);
            assert_eq!(completed.published, matches!(operation, Operation::Publish));
            assert_eq!(
                completed.quarantined,
                matches!(operation, Operation::Quarantine)
            );
            assert_eq!(completed.lease_generation, leased.lease_generation);
            assert_eq!(
                completed.last_failure_code.as_deref(),
                (!completed.published).then_some(FAILURE.as_str())
            );
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn deferred_commit_rejection_is_an_invariant_and_preserves_evidence() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        seed(&mut worker, "commit-rejection-message").await?;
        let claimed = claim(&mut worker).await?;
        let before = state(&mut observer, claimed.message_id()).await?;
        sqlx::raw_sql(
            r"
            CREATE FUNCTION reject_publication_at_commit()
            RETURNS TRIGGER LANGUAGE plpgsql AS $$
            BEGIN
                IF NEW.published_at IS NOT NULL THEN
                    RAISE EXCEPTION USING
                        ERRCODE = '23514',
                        MESSAGE = 'private-commit-sentinel-7391';
                END IF;
                RETURN NEW;
            END;
            $$;
            CREATE CONSTRAINT TRIGGER reject_publication_at_commit
                AFTER UPDATE ON edgeagent_message_outbox
                DEFERRABLE INITIALLY DEFERRED
                FOR EACH ROW EXECUTE FUNCTION reject_publication_at_commit();
            ",
        )
        .execute(&mut worker)
        .await?;
        let error = apply(&mut worker, Operation::Publish, &claimed)
            .await
            .err()
            .ok_or("deferred publication unexpectedly committed")?;
        assert_invariant(&error);
        let adapter = error
            .source()
            .and_then(|cause| cause.downcast_ref::<OutboxError>())
            .ok_or("missing concrete outbox error")?;
        let driver = adapter
            .source()
            .and_then(|cause| cause.downcast_ref::<sqlx::Error>())
            .ok_or("missing concrete SQLx error")?;
        let sqlx::Error::Database(database) = driver else {
            return Err("expected the PostgreSQL deferred rejection".into());
        };
        assert_eq!(database.code().as_deref(), Some("23514"));
        assert!(database.message().contains("private-commit-sentinel-7391"));
        for rendered in [
            error.to_string(),
            format!("{error:?}"),
            adapter.to_string(),
            format!("{adapter:?}"),
        ] {
            assert!(!rendered.contains("private-commit-sentinel-7391"));
        }
        // Connection reuse flushes any queued rollback before observing recovery.
        sqlx::query("SELECT 1").execute(&mut worker).await?;
        assert_eq!(state(&mut observer, claimed.message_id()).await?, before);
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn terminated_connection_is_unavailable_through_relay_port() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut worker)
            .await?;
        // A nonzero timeout waits for exit, rather than merely reporting that
        // the termination signal was sent before the next relay operation.
        let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1, 2000)")
            .bind(backend)
            .fetch_one(&mut observer)
            .await?;
        assert!(terminated);
        let error = PostgresOutboxRelay::new(&mut worker)
            .claim_one(OWNER, LeaseDuration::new(LeaseDuration::MAX)?)
            .await
            .err()
            .ok_or("terminated PostgreSQL connection unexpectedly succeeded")?;
        assert_eq!(error.kind(), OutboxStoreErrorKind::Unavailable);
        assert!(error.source().and_then(Error::source).is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn invalid_stored_claim_rolls_back_before_connection_reuse() -> TestResult {
    with_fixture(|fixture| async move {
        let mut worker = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        seed(&mut worker, "invalid-stored-message").await?;
        // Only this owned test schema permits an invalid counter. The UPDATE
        // succeeds and returns -1; conversion then fails in Rust before commit.
        sqlx::raw_sql(
            r"
            ALTER TABLE edgeagent_message_outbox
                DROP CONSTRAINT edgeagent_message_outbox_lease_generation_nonnegative;
            UPDATE edgeagent_message_outbox SET lease_generation = -2;
            ",
        )
        .execute(&mut worker)
        .await?;
        let before = state(&mut observer, "invalid-stored-message").await?;
        let error = PostgresOutboxRelay::new(&mut worker)
            .claim_one(OWNER, LeaseDuration::new(LeaseDuration::MAX)?)
            .await
            .err()
            .ok_or("invalid stored generation unexpectedly produced a claim")?;
        assert_invariant(&error);
        assert!(error.source().and_then(Error::source).is_some());
        sqlx::query("SELECT 1").execute(&mut worker).await?;
        assert_eq!(
            state(&mut observer, "invalid-stored-message").await?,
            before
        );
        assert_eq!(before.attempt_count, 0);
        assert_eq!(before.lease_owner, None);
        assert_eq!(before.lease_generation, -2);
        Ok(())
    })
    .await
}
