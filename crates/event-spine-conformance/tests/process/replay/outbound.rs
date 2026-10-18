use super::super::{plan, scenarios::provision_broker};
use super::*;
use edgeagent_messaging::{
    FailureCode, LeaseDuration, OutboxRelayStore, OutboxRetryDelay, OutboxStoreErrorKind,
};
use edgeagent_outbox_postgres::PostgresOutboxRelay;
use sqlx::ConnectOptions;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn outbound_replay_preserves_audit_budget_and_stale_claim_fences() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| Box::pin(async move {
            assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
            let mut connection = sqlx::PgConnection::connect_with(&db.runtime_options(false, true).options([("search_path", db.producer.as_str())])).await?;
            let lease = LeaseDuration::new(Duration::from_secs(10))?;
            let mut relay = PostgresOutboxRelay::new(&mut connection);
            let stale = relay.claim_one("same_relay", lease).await?.ok_or("claim absent")?;
            sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {}.edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - INTERVAL '1 second'", db.producer))).execute(&mut db.admin).await?;
            let quarantined = relay.claim_one("same_relay", lease).await?.ok_or("reclaim absent")?;
            relay.quarantine(&quarantined, "same_relay", FailureCode::from_static("fixture_failure")).await?;
            let request = ReplayCommand::Outbound {
                request_id: "outbound-01".into(), message_source: stale.message_source().into(), message_id: stale.message_id().into(), reason: "dependency_remediated".into()
            };
            let mut denied = database::Worker::spawn("replay-authorize", db.runtime_options(false, false).to_url_lossy().as_str(), &db.producer, "gateway", &url).await?;
            denied.begin(b'c', &serde_json::to_vec(&request)?).await?;
            assert!(denied.finish().await.is_err());
            assert_eq!(authorize(db, &url, false, "gateway", &request, false).await?, "rolled_back");
            assert_eq!(db.count(false, "edgeagent_message_outbox_replay_audit").await?, 0);
            let before: (i32, i64, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT attempt_count, lease_generation, quarantined_at IS NOT NULL FROM {}.edgeagent_message_outbox", db.producer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!(before, (2, 2, true));
            assert_eq!(authorize(db, &url, false, "gateway", &request, true).await?, "released");
            assert_eq!(authorize(db, &url, false, "gateway", &request, true).await?, "already_released");
            let after: (i32, i64, bool) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT attempt_count, lease_generation, quarantined_at IS NOT NULL FROM {}.edgeagent_message_outbox", db.producer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!(after, (0, 2, false), "replay resets attempts, not the fencing generation");
            let conflict = ReplayCommand::Outbound { request_id: "outbound-01".into(), message_source: stale.message_source().into(), message_id: stale.message_id().into(), reason: "changed_reason".into() };
            assert!(authorize(db, &url, false, "gateway", &conflict, true).await.is_err());
            let fresh = relay.claim_one("same_relay", lease).await?.ok_or("released claim absent")?;
            assert!(fresh.lease_generation().get() > quarantined.lease_generation().get());
            for old in [&stale, &quarantined] {
                let error = relay.mark_published(old, "same_relay").await.err().ok_or("stale fence accepted")?;
                assert_eq!(error.kind(), OutboxStoreErrorKind::StateTransition);
            }
            relay.release_for_retry(&fresh, "same_relay", OutboxRetryDelay::IMMEDIATE, FailureCode::from_static("fixture_retry")).await?;
            connection.close().await?;
            assert_eq!(db.worker("relay", false, &url, true).await?, "published");
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            let (actor, prior_attempts, reason): (String, i32, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT requested_by, prior_attempt_count, prior_quarantine_reason FROM {}.edgeagent_message_outbox_replay_audit", db.producer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!((actor, prior_attempts, reason), (format!("{}_o", db.producer), 2, "fixture_failure".into()));
            let retained: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT envelope FROM {}.edgeagent_message_outbox", db.producer))).fetch_one(&mut db.admin).await?;
            assert_eq!(retained, stale.envelope_bytes());
            assert_eq!(db.count(false, "edgeagent_message_outbox_replay_audit").await?, 1);
            Ok(())
        })).await
    }).await
}
