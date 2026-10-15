use super::*;

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn competing_claims_skip_locked_and_bound_batches() -> TestResult {
    with_fixture(|fixture| async move {
        let mut first = fixture.connect().await?;
        let mut second = fixture.connect().await?;
        for id in ["batch-a", "batch-b", "batch-c", "batch-d"] {
            seed(&mut first, id).await?;
        }
        let lease = LeaseDuration::new(LeaseDuration::MAX)?;
        let mut first_tx = first.begin().await?;
        let first_claims = PostgresOutbox
            .claim_batch(&mut first_tx, "first_worker", 1, lease)
            .await?;
        assert_eq!(first_claims.len(), 1);
        // Keep the first transaction open: SKIP LOCKED, not just committed lease
        // filtering, must make progress and produce disjoint claims.
        let mut second_tx = second.begin().await?;
        let second_claims = PostgresOutbox
            .claim_batch(&mut second_tx, "second_worker", 2, lease)
            .await?;
        assert_eq!(second_claims.len(), 2);
        let first_ids = first_claims
            .iter()
            .map(ClaimedMessage::message_id)
            .collect::<BTreeSet<_>>();
        let second_ids = second_claims
            .iter()
            .map(ClaimedMessage::message_id)
            .collect::<BTreeSet<_>>();
        assert!(first_ids.is_disjoint(&second_ids));
        second_tx.commit().await?;
        first_tx.commit().await?;
        let mut transaction = second.begin().await?;
        let remaining = PostgresOutbox
            .claim_batch(&mut transaction, OWNER, 2, lease)
            .await?;
        assert_eq!(remaining.len(), 1);
        assert!(!first_ids.contains(remaining[0].message_id()));
        assert!(!second_ids.contains(remaining[0].message_id()));
        transaction.commit().await?;
        // Assert set membership/counts, never UPDATE RETURNING or FIFO order.
        let mut transaction = first.begin().await?;
        assert!(
            PostgresOutbox
                .claim_batch(&mut transaction, OWNER, 2, lease)
                .await?
                .is_empty()
        );
        transaction.commit().await?;
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn waiting_enqueues_observe_committed_identity_decisions() -> TestResult {
    with_fixture(|fixture| async move {
        for (id, quantity) in [("identical", 1), ("conflicting", 2)] {
            competing_enqueue(&fixture, id, quantity, true).await?;
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn waiting_enqueue_inserts_after_competing_rollback() -> TestResult {
    with_fixture(
        |fixture| async move { competing_enqueue(&fixture, "rolled-back", 2, false).await },
    )
    .await
}

async fn competing_enqueue(fixture: &Fixture, id: &str, quantity: u16, commit: bool) -> TestResult {
    let mut first = fixture.connect().await?;
    let mut second = fixture.connect().await?;
    let mut observer = fixture.connect().await?;
    let blocker = backend_pid(&mut first).await?;
    let waiter = backend_pid(&mut second).await?;
    let original = envelope(id, 1)?;
    let competing = envelope(id, quantity)?;
    let mut first_tx = first.begin().await?;
    assert_eq!(
        PostgresOutbox
            .enqueue(&mut first_tx, COMMAND, &original)
            .await?,
        EnqueueDisposition::Inserted
    );
    let mut second_tx = second.begin().await?;
    let mut pending = Box::pin(PostgresOutbox.enqueue(&mut second_tx, COMMAND, &competing));
    tokio::select! {
        result = &mut pending => return Err(format!("enqueue completed before competitor release: {result:?}").into()),
        blocked = observe_blocked(&mut observer, waiter, blocker) => blocked?,
    }
    if commit {
        first_tx.commit().await?;
    } else {
        first_tx.rollback().await?;
    }
    let result = pending.await;
    if !commit {
        assert_eq!(result?, EnqueueDisposition::Inserted);
    } else if quantity == 1 {
        assert_eq!(result?, EnqueueDisposition::AlreadyPresent);
    } else {
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(OutboxErrorKind::MessageIdentityConflict)
        );
    }
    second_tx.commit().await?;
    let expected_bytes = if commit {
        original.to_json()?
    } else {
        competing.to_json()?
    };
    let (count, matches): (i64, bool) = sqlx::query_as(
        "SELECT count(*), bool_and(envelope = $2)
         FROM edgeagent_message_outbox WHERE message_id = $1",
    )
    .bind(id)
    .bind(expected_bytes)
    .fetch_one(&mut observer)
    .await?;
    assert_eq!(count, 1);
    assert!(matches);
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn competing_replays_release_and_audit_only_once() -> TestResult {
    with_fixture(|fixture| async move {
        for (id, second_request) in [
            ("same-replay", "first-replay"),
            ("other-replay", "competing-replay"),
        ] {
            let mut first = fixture.connect().await?;
            let mut second = fixture.connect().await?;
            let mut observer = fixture.connect().await?;
            seed(&mut first, id).await?;
            let claim = claim(&mut first).await?;
            let mut transaction = first.begin().await?;
            PostgresOutbox.quarantine(&mut transaction, &claim, OWNER, FAILURE).await?;
            transaction.commit().await?;
            // Request identities are unique across cases; only the first case
            // intentionally shares the request between concurrent contenders.
            let first_id = format!("{id}-first-replay");
            let second_id = format!("{id}-{second_request}");
            let blocker = backend_pid(&mut first).await?;
            let waiter = backend_pid(&mut second).await?;
            let mut first_tx = first.begin().await?;
            assert_eq!(
                PostgresOutbox.replay_quarantined(
                    &mut first_tx, SOURCE, id, replay_request(&first_id)?,
                ).await?,
                ReplayDisposition::Released,
            );
            let mut second_tx = second.begin().await?;
            let mut pending = Box::pin(PostgresOutbox.replay_quarantined(
                &mut second_tx, SOURCE, id, replay_request(&second_id)?,
            ));
            tokio::select! {
                result = &mut pending => return Err(format!("replay completed before competitor release: {result:?}").into()),
                blocked = observe_blocked(&mut observer, waiter, blocker) => blocked?,
            }
            first_tx.commit().await?;
            let result = pending.await;
            if first_id == second_id {
                assert_eq!(result?, ReplayDisposition::AlreadyRequested);
            } else {
                assert_eq!(
                    result.err().map(|error| error.kind()),
                    Some(OutboxErrorKind::NotQuarantined),
                );
            }
            second_tx.commit().await?;
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM edgeagent_message_outbox_replay_audit
                 WHERE message_id = $1",
            ).bind(id).fetch_one(&mut observer).await?;
            assert_eq!(count, 1);
            let (released, attempt, generation): (bool, i32, i64) = sqlx::query_as(
                "SELECT quarantined_at IS NULL, attempt_count, lease_generation
                 FROM edgeagent_message_outbox WHERE message_id = $1",
            ).bind(id).fetch_one(&mut observer).await?;
            assert!(released);
            assert_eq!(attempt, 0);
            assert_eq!(u64::try_from(generation)?, claim.lease_generation().get());
            // Resolve this case so the next seed is the only eligible record.
            let current = super::claim(&mut first).await?;
            let mut transaction = first.begin().await?;
            PostgresOutbox.mark_published(&mut transaction, &current, OWNER).await?;
            transaction.commit().await?;
        }
        Ok(())
    }).await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn nonzero_retry_delay_defers_claim_until_persisted_eligibility() -> TestResult {
    with_fixture(|fixture| async move {
        let mut writer = fixture.connect().await?;
        let mut observer = fixture.connect().await?;
        seed(&mut writer, "delayed").await?;
        let claim = claim(&mut writer).await?;
        let mut transaction = writer.begin().await?;
        PostgresOutbox
            .release_for_retry(
                &mut transaction,
                &claim,
                OWNER,
                OutboxRetryDelay::new(Duration::from_secs(3600))?,
                FAILURE,
            )
            .await?;
        transaction.commit().await?;
        let future: bool = sqlx::query_scalar(
            "SELECT available_at > clock_timestamp() AND lease_owner IS NULL
             FROM edgeagent_message_outbox WHERE message_id = 'delayed'",
        )
        .fetch_one(&mut observer)
        .await?;
        assert!(future);
        let mut transaction = observer.begin().await?;
        let premature = PostgresOutbox
            .claim_batch(
                &mut transaction,
                OWNER,
                1,
                LeaseDuration::new(LeaseDuration::MAX)?,
            )
            .await?;
        assert!(premature.is_empty());
        transaction.commit().await?;
        // Advance persisted eligibility, not the test's wall clock.
        sqlx::query(
            "UPDATE edgeagent_message_outbox
             SET available_at = clock_timestamp() - INTERVAL '1 second'
             WHERE message_id = 'delayed'",
        )
        .execute(&mut writer)
        .await?;
        let next = super::claim(&mut observer).await?;
        assert_eq!(next.message_id(), "delayed");
        assert_eq!(next.attempt(), 2);
        assert!(next.lease_generation().get() > claim.lease_generation().get());
        Ok(())
    })
    .await
}
