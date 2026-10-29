//! Real atomicity proofs in unique owned PostgreSQL schemas.
#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema inspection and fault injection."
)]
mod support;
use edgeagent_research_application::publication::{
    PublicationErrorKind as Kind, ResearchPublication,
};
use edgeagent_research_postgres::PostgresResearchStore;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn duplicate_requests_return_first_outcome_and_conflicting_intent_rolls_back() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let first = prepared("scope", "request-1", "delivery-1", 110, 3)?;
        assert!(store.recover(first.delivery()).await?.is_none());
        assert_eq!(h.counts().await?, (0, 0, 0));
        let receipt = store.commit(&first).await?;
        let different = prepared("scope", "request-1", "delivery-2", 120, 3)?;
        assert_eq!(store.commit(&different).await?.bytes(), receipt.bytes());
        assert_eq!(store.commit(&first).await?.bytes(), receipt.bytes());
        let changed = prepared("scope", "request-1", "delivery-3", 110, 9)?;
        assert_eq!(
            store
                .commit(&changed)
                .await
                .err()
                .ok_or("expected conflict")?
                .kind(),
            Kind::Conflict
        );
        assert_eq!(h.counts().await?, (1, 2, 1));
        // Same local ID in another authenticated caller scope is independent.
        store
            .commit(&prepared(
                "another-scope",
                "request-1",
                "delivery-4",
                110,
                3,
            )?)
            .await?;
        assert_eq!(h.counts().await?, (2, 3, 2));
        let denied = abstained("scope", "abstained", "delivery-5")?;
        let receipt = store.commit(&denied).await?;
        assert!(matches!(
            receipt.record(),
            edgeagent_contracts::research::ResearchRecord::Abstention(_)
        ));
        assert_eq!(
            receipt.publication().message_type(),
            edgeagent_research_application::publication::ABSTAINED.message_type()
        );
        assert_eq!(store.commit(&denied).await?.bytes(), receipt.bytes());
        assert_eq!(h.counts().await?, (3, 4, 3));
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn concurrent_completion_and_client_restart_recover_exact_original_receipt() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let a = prepared("scope", "request-1", "delivery-1", 110, 3)?;
        let b = prepared("scope", "request-1", "delivery-2", 120, 3)?;
        let (one, two) = tokio::join!(store.commit(&a), store.commit(&b));
        let one = one?;
        let two = two?;
        assert_eq!(one.bytes(), two.bytes());
        assert_eq!(one.publication().to_json()?, two.publication().to_json()?);
        let pool = h.reconnect().await?;
        let restarted = PostgresResearchStore::new(pool.clone());
        // Receipt-first recovery needs only the original delivery, never recalculation.
        let retained = restarted
            .recover(a.delivery())
            .await?
            .ok_or("lost receipt")?;
        assert_eq!(retained.bytes(), one.bytes());
        assert_eq!(
            retained.publication().to_json()?,
            one.publication().to_json()?
        );
        pool.close().await;
        assert_eq!(h.counts().await?, (1, 2, 1));
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn statement_and_deferred_commit_failures_rollback_completion_inbox_and_outbox() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let store=PostgresResearchStore::new(h.pool.clone());let p=prepared("scope","request-1","delivery-1",110,3)?;
        sqlx::raw_sql("CREATE FUNCTION test_refuse_research() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-test-sentinel'; END; $$").execute(&h.pool).await?;
        for trigger in ["CREATE TRIGGER test_refuse BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION test_refuse_research()","CREATE CONSTRAINT TRIGGER test_refuse AFTER INSERT ON edgeagent_message_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION test_refuse_research()"] {
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            let error=store.commit(&p).await.err().ok_or("expected rollback")?;
            assert_eq!(error.kind(),Kind::Storage);assert!(!format!("{error:?} {error}").contains("private-test-sentinel"));
            assert_eq!(h.counts().await?,(0,0,0));assert!(store.recover(p.delivery()).await?.is_none());
            sqlx::raw_sql("DROP TRIGGER test_refuse ON edgeagent_message_outbox").execute(&h.pool).await?;
        }
        store.commit(&p).await?;assert_eq!(h.counts().await?,(1,1,1));Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn cancelled_transaction_leaves_no_completion_or_publication_intent() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let mut barrier=h.pool.acquire().await?;
        sqlx::query("SELECT pg_advisory_lock($1)").bind(h.key).execute(&mut *barrier).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION test_research_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({}); RETURN NEW; END; $$; CREATE TRIGGER test_barrier BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION test_research_barrier()",h.key))).execute(&h.pool).await?;
        let pool=h.pool.clone();let p=prepared("scope","request-1","delivery-1",110,3)?;
        let worker=tokio::spawn(async move {PostgresResearchStore::new(pool).commit(&p).await});
        let waiting=tokio::time::timeout(std::time::Duration::from_secs(3),async {
            loop {let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND NOT granted AND classid::bigint=$1 AND objid::bigint=$2)").bind(h.key>>32).bind(h.key & i64::from(u32::MAX)).fetch_one(&h.pool).await?;if blocked {return Ok::<_,sqlx::Error>(());}tokio::task::yield_now().await;}
        }).await;
        worker.abort();let _=worker.await;
        sqlx::query("SELECT pg_advisory_unlock($1)").bind(h.key).execute(&mut *barrier).await?;
        // Queue after the worker; reacquisition waits for its dropped transaction to roll back.
        sqlx::query("SELECT pg_advisory_lock($1)").bind(h.key).execute(&mut *barrier).await?;
        sqlx::query("SELECT pg_advisory_unlock($1)").bind(h.key).execute(&mut *barrier).await?;
        waiting??;drop(barrier);assert_eq!(h.counts().await?,(0,0,0));Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn immutable_completion_and_corrupt_retained_outbox_fail_closed() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let p = prepared("scope", "request-1", "delivery-1", 110, 3)?;
        store.commit(&p).await?;
        for sql in [
            "UPDATE edgeagent_research_completions SET outcome=outcome",
            "DELETE FROM edgeagent_research_completions",
        ] {
            assert!(sqlx::query(sql).execute(&h.pool).await.is_err());
        }
        let reused = prepared("scope", "request-2", "delivery-1", 110, 3)?;
        assert_eq!(
            store
                .commit(&reused)
                .await
                .err()
                .ok_or("expected delivery conflict")?
                .kind(),
            Kind::Conflict
        );
        sqlx::query("UPDATE edgeagent_message_outbox SET transport_subject='changed'")
            .execute(&h.pool)
            .await?;
        let routed_retry = prepared("scope", "request-1", "delivery-routing", 110, 3)?;
        assert_eq!(
            store
                .recover(routed_retry.delivery())
                .await
                .err()
                .ok_or("expected routing integrity failure")?
                .kind(),
            Kind::Invalid
        );
        assert_eq!(h.counts().await?, (1, 1, 1));
        sqlx::query("DELETE FROM edgeagent_message_outbox")
            .execute(&h.pool)
            .await?;
        let retry = prepared("scope", "request-1", "delivery-2", 110, 3)?;
        assert_eq!(
            store
                .recover(retry.delivery())
                .await
                .err()
                .ok_or("expected integrity failure")?
                .kind(),
            Kind::Invalid
        );
        assert_eq!(h.counts().await?, (1, 1, 0));
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
fn life(
    receipt: &edgeagent_research_application::publication::CommittedOutcome,
    revision: u64,
    change: edgeagent_contracts::research::LifecycleChange,
) -> Result<edgeagent_contracts::research::ResearchLifecycle> {
    let edgeagent_contracts::research::ResearchRecord::Artifact(a) = receipt.record() else {
        return Err("artifact".into());
    };
    Ok(edgeagent_contracts::research::ResearchLifecycle {
        scope: a.request.scope.clone(),
        request_id: a.request.request_id.clone(),
        artifact: receipt.record().digest()?,
        revision,
        effective_at_ms: 100,
        change,
    })
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn lifecycle_commands_are_ordered_idempotent_and_corrections_cannot_cycle() -> Result {
    use edgeagent_contracts::research::LifecycleChange;
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let a = store
            .commit(&prepared("scope", "original", "a", 110, 3)?)
            .await?;
        let b = store
            .commit(&prepared("scope", "replacement", "b", 120, 9)?)
            .await?;
        let original = a.bytes().to_vec();
        let event = life(&a, 1, LifecycleChange::Invalidated)?;
        let (one, two) = tokio::join!(
            store.append_lifecycle("scope", event.clone(), TIME),
            store.append_lifecycle("scope", event.clone(), TIME)
        );
        assert_eq!(one?.to_json()?, two?.to_json()?);
        let mut conflict = event.clone();
        conflict.change = LifecycleChange::Withdrawn;
        assert_eq!(
            store
                .append_lifecycle("scope", conflict, TIME)
                .await
                .err()
                .ok_or("conflict")?
                .kind(),
            Kind::Conflict
        );
        let correction = life(
            &a,
            2,
            LifecycleChange::Corrected {
                replacement: b.record().digest()?,
            },
        )?;
        let appended = store
            .append_lifecycle("scope", correction.clone(), TIME)
            .await?;
        assert!(edgeagent_research_application::lifecycle::decode(&appended)? == correction);
        assert_eq!(
            store
                .append_lifecycle("scope", correction, TIME)
                .await?
                .to_json()?,
            appended.to_json()?
        );
        assert!(
            store
                .append_lifecycle("foreign", event, TIME)
                .await
                .is_err()
        );
        assert!(
            store
                .append_lifecycle(
                    "scope",
                    life(
                        &b,
                        1,
                        LifecycleChange::Corrected {
                            replacement: a.record().digest()?
                        }
                    )?,
                    TIME
                )
                .await
                .is_err()
        );
        assert_eq!(
            store
                .commit(&prepared("scope", "original", "again", 110, 3)?)
                .await?
                .bytes(),
            original
        );
        for (id, change, time) in [
            ("expired", LifecycleChange::Expired, 101),
            ("withdrawn", LifecycleChange::Withdrawn, 100),
        ] {
            let receipt = store.commit(&prepared("scope", id, id, 110, 3)?).await?;
            let mut command = life(&receipt, 1, change)?;
            command.effective_at_ms = time;
            if id == "expired" {
                let mut early = command.clone();
                early.effective_at_ms = 100;
                assert!(store.append_lifecycle("scope", early, TIME).await.is_err());
            }
            store.append_lifecycle("scope", command, TIME).await?;
        }
        let missing = life(
            &b,
            1,
            LifecycleChange::Corrected {
                replacement: "09".repeat(32),
            },
        )?;
        assert!(
            store
                .append_lifecycle("scope", missing, TIME)
                .await
                .is_err()
        );
        assert!(
            store
                .append_lifecycle("scope", life(&b, 2, LifecycleChange::Withdrawn)?, TIME)
                .await
                .is_err()
        );
        let x = store
            .commit(&prepared("scope", "cycle-x", "x", 110, 3)?)
            .await?;
        let y = store
            .commit(&prepared("scope", "cycle-y", "y", 110, 3)?)
            .await?;
        let xy = life(
            &x,
            1,
            LifecycleChange::Corrected {
                replacement: y.record().digest()?,
            },
        )?;
        let yx = life(
            &y,
            1,
            LifecycleChange::Corrected {
                replacement: x.record().digest()?,
            },
        )?;
        let (left, right) = tokio::join!(
            store.append_lifecycle("scope", xy, TIME),
            store.append_lifecycle("scope", yx, TIME)
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let foreign = store
            .commit(&prepared("foreign", "replacement", "foreign", 110, 3)?)
            .await?;
        assert!(
            store
                .append_lifecycle(
                    "scope",
                    life(
                        &b,
                        1,
                        LifecycleChange::Corrected {
                            replacement: foreign.record().digest()?
                        }
                    )?,
                    TIME
                )
                .await
                .is_err()
        );
        let target = store
            .commit(&prepared("scope", "expired-target", "target", 110, 3)?)
            .await?;
        let mut late = life(
            &b,
            1,
            LifecycleChange::Corrected {
                replacement: target.record().digest()?,
            },
        )?;
        late.effective_at_ms = 101;
        assert!(store.append_lifecycle("scope", late, TIME).await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_lifecycle")
                .fetch_one(&h.pool)
                .await?,
            5
        );
        assert!(
            sqlx::raw_sql("UPDATE edgeagent_research_lifecycle SET revision=revision")
                .execute(&h.pool)
                .await
                .is_err()
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn lifecycle_statement_and_deferred_commit_failures_leave_no_revision_or_outbox() -> Result {
    use edgeagent_contracts::research::LifecycleChange;
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let store=PostgresResearchStore::new(h.pool.clone());
        let receipt=store.commit(&prepared("scope","original","a",110,3)?).await?;
        let event=life(&receipt,1,LifecycleChange::Withdrawn)?;
        for trigger in ["CREATE TRIGGER lifecycle_fault BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION lifecycle_fault()","CREATE CONSTRAINT TRIGGER lifecycle_fault AFTER INSERT ON edgeagent_research_lifecycle DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION lifecycle_fault()"] {
            sqlx::raw_sql("CREATE OR REPLACE FUNCTION lifecycle_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic lifecycle fault'; END $$").execute(&h.pool).await?;
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            assert_eq!(store.append_lifecycle("scope",event.clone(),TIME).await.err().ok_or("failure")?.kind(),Kind::Storage);
            assert_eq!(h.counts().await?,(1,1,1));
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_lifecycle").fetch_one(&h.pool).await?,0);
            sqlx::raw_sql(if trigger.starts_with("CREATE CONSTRAINT") {"DROP TRIGGER lifecycle_fault ON edgeagent_research_lifecycle"} else {"DROP TRIGGER lifecycle_fault ON edgeagent_message_outbox"}).execute(&h.pool).await?;
        }
        store.append_lifecycle("scope",event,TIME).await?;assert_eq!(h.counts().await?,(1,1,2));Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
