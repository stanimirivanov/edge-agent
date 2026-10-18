use super::super::{
    plan,
    scenarios::{command, provision_broker, state},
};
use super::*;
use edgeagent_contracts::MessageEnvelope;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn inbound_replay_recovers_persisted_result_loss_with_one_domain_transition() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| Box::pin(async move {
            command(db, &url, "successor", "replay", 1, 43).await?;
            for attempt in 1..=5 {
                assert_eq!(db.worker("consumer", true, &url, true).await?, if attempt < 5 { "deferred" } else { "quarantined:missing_predecessor" });
            }
            let (consumer, key, original): (String, String, Vec<u8>) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT consumer_name, delivery_key, payload FROM {}.edgeagent_message_quarantine", db.consumer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!(MessageEnvelope::from_json(&original)?.id(), "successor");
            command(db, &url, "predecessor", "replay", 0, 42).await?;
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            assert_eq!(authorize(db, &url, true, "research", &inbound("recover-01", &consumer, &key), true).await?, "authorized");
            assert_eq!(db.count(true, "conformance_replay_receipt").await?, 0);
            // Authorizer has exited. A separate dispatcher persists a real
            // broker result, then is killed before recording that result.
            let mut lost = db.replay_worker(true, true, "research", &url).await?;
            lost.begin(b'p', &[]).await?;
            lost.expect("persisted").await?;
            let transaction_state: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
                "SELECT state FROM pg_stat_activity WHERE application_name = $1"
            )).bind(lost.application_name()?).fetch_one(&mut db.admin).await?;
            assert_eq!(transaction_state, "idle", "publication barrier holds no SQL transaction");
            let published: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT published_at IS NOT NULL FROM {}.conformance_replay_dispatch", db.consumer
            ))).fetch_one(&mut db.admin).await?;
            assert!(!published);
            lost.kill().await?;
            expire(db).await?;
            assert_eq!(dispatch(db, &url, "research").await?, "replay_duplicate");
            let (attempts, generation, duplicate): (i32, i64, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT d.attempt_count, d.generation, r.duplicate FROM {}.conformance_replay_dispatch d JOIN {}.conformance_replay_receipt r USING (request_id)", db.consumer, db.consumer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!((attempts, generation, duplicate), (2, 2, true));
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            assert_eq!(state(db, "replay").await?, Some((2, 43)));
            assert_eq!(db.count(true, "conformance_command").await?, 2);
            // A stalled holder cannot record over a newer generation, even
            // when both dispatchers authenticate as the same service login.
            assert_eq!(authorize(db, &url, true, "research", &inbound("fenced", &consumer, &key), true).await?, "authorized");
            let mut stale = db.replay_worker(true, true, "research", &url).await?;
            stale.begin(b'p', &[]).await?;
            stale.expect("persisted").await?;
            assert_eq!(dispatch(db, &url, "research").await?, "idle", "an active lease excludes another claim");
            expire(db).await?;
            let mut current = db.replay_worker(true, true, "research", &url).await?;
            current.begin(b'p', &[]).await?;
            current.expect("persisted").await?;
            // Try the stale completion while generation two is still leased
            // and unpublished. A terminal-state check alone cannot pass this.
            stale.begin(b'c', &[]).await?;
            assert!(stale.finish().await.is_err());
            current.begin(b'c', &[]).await?;
            assert_eq!(current.finish().await?, "replay_duplicate");
            let fence: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT generation FROM {}.conformance_replay_receipt WHERE request_id = 'fenced'", db.consumer))).fetch_one(&mut db.admin).await?;
            assert_eq!(fence, 2);
            assert_eq!(db.worker("consumer", true, &url, true).await?, "duplicate");
            // Unknown outcomes consume the same bounded attempt budget; a
            // restart must not reset it or silently mark exhausted work done.
            assert_eq!(authorize(db, &url, true, "research", &inbound("exhausted", &consumer, &key), true).await?, "authorized");
            for _ in 0..3 {
                let mut lost = db.replay_worker(true, true, "research", &url).await?;
                lost.begin(b'p', &[]).await?;
                lost.expect("persisted").await?;
                lost.kill().await?;
                expire(db).await?;
            }
            assert_eq!(dispatch(db, &url, "research").await?, "idle");
            let exhausted: (i32, i64, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT attempt_count, generation, published_at IS NULL FROM {}.conformance_replay_dispatch WHERE request_id = 'exhausted'", db.consumer))).fetch_one(&mut db.admin).await?;
            assert_eq!(exhausted, (3, 3, true));
            assert_eq!(db.count(true, "conformance_replay_receipt").await?, 2);
            assert_eq!(db.worker("consumer", true, &url, true).await?, "duplicate");
            assert_eq!(db.count(true, "conformance_command").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_quarantine").await?, 1);
            // Idempotent request re-entry cannot create another dispatch.
            assert_eq!(authorize(db, &url, true, "research", &inbound("recover-01", &consumer, &key), true).await?, "authorized");
            assert_eq!(dispatch(db, &url, "research").await?, "idle");
            // An intentional further replay has a fresh transport identity,
            // while the original source/ID causes a durable inbox duplicate.
            assert_eq!(authorize(db, &url, true, "research", &inbound("recover-02", &consumer, &key), true).await?, "authorized");
            assert_eq!(dispatch(db, &url, "research").await?, "replayed");
            assert_eq!(db.worker("consumer", true, &url, true).await?, "duplicate");
            assert_eq!(db.count(true, "conformance_command").await?, 2);
            let snapshots: Vec<Vec<u8>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT payload FROM {}.conformance_replay_dispatch", db.consumer))).fetch_all(&mut db.admin).await?;
            assert_eq!(snapshots.len(), 4);
            assert!(snapshots.iter().all(|bytes| bytes == &original));
            assert_eq!(db.count(true, "conformance_replay_receipt").await?, 3);
            Ok(())
        })).await
    }).await
}
