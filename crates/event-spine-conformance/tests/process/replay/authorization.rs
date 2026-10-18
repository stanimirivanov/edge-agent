use super::super::{plan, scenarios::provision_broker};
use super::*;
use edgeagent_event_spine_conformance::definitions;
use sqlx::ConnectOptions;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn authenticated_replay_authorization_is_scoped_atomic_and_immutable() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| Box::pin(async move {
            let consumer = consumer_name(Component::Research);
            let subject = definitions()[0].subject()?;
            let bytes = include_bytes!("../../../fixtures/v1/apply-probe.json");
            retain(db, &consumer, "valid", &subject, bytes).await?;
            let request = inbound("auth-01", &consumer, "valid");
            // A real ordinary runtime login cannot self-declare operator status.
            let mut denied = database::Worker::spawn("replay-authorize",
                db.runtime_options(true, false).to_url_lossy().as_str(), &db.consumer, "research", &url).await?;
            denied.begin(b'c', &serde_json::to_vec(&request)?).await?;
            assert!(denied.finish().await.is_err());
            assert_eq!(db.count(true, "edgeagent_message_quarantine_replay_audit").await?, 0);
            assert_eq!(db.count(true, "conformance_replay_dispatch").await?, 0);

            assert_eq!(authorize(db, &url, true, "research", &request, false).await?, "rolled_back");
            assert_eq!(db.count(true, "edgeagent_message_quarantine_replay_audit").await?, 0);
            assert_eq!(db.count(true, "conformance_replay_dispatch").await?, 0);
            for _ in 0..2 {
                assert_eq!(authorize(db, &url, true, "research", &request, true).await?, "authorized");
            }
            assert_eq!(db.count(true, "edgeagent_message_quarantine_replay_audit").await?, 1);
            assert_eq!(db.count(true, "conformance_replay_dispatch").await?, 1);
            let (actor, payload): (String, Vec<u8>) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT a.requested_by, d.payload FROM {}.edgeagent_message_quarantine_replay_audit a JOIN {}.conformance_replay_dispatch d ON d.request_id = a.replay_request_id", db.consumer, db.consumer
            ))).fetch_one(&mut db.admin).await?;
            assert_eq!(actor, format!("{}_o", db.consumer));
            assert_eq!(payload, bytes);
            let conflicting = ReplayCommand::Inbound { request_id: "auth-01".into(), consumer_name: consumer.clone(), delivery_key: "valid".into(), reason: "different_reason".into() };
            assert!(authorize(db, &url, true, "research", &conflicting, true).await.is_err());
            retain(db, &consumer, "other-valid", &subject, bytes).await?;
            assert!(authorize(db, &url, true, "research", &inbound("auth-01", &consumer, "other-valid"), true).await.is_err());

            let mut forged = serde_json::to_value(&request)?;
            forged["requested_by"] = "administrator".into();
            let mut worker = db.replay_worker(true, false, "research", &url).await?;
            worker.begin(b'c', &serde_json::to_vec(&forged)?).await?;
            assert!(worker.finish().await.is_err());
            // The same authenticated operator cannot select a peer schema.
            let mut peer = database::Worker::spawn("replay-authorize",
                db.replay_options(true, false).to_url_lossy().as_str(), &db.producer, "research", &url).await?;
            peer.begin(b'c', &serde_json::to_vec(&request)?).await?;
            assert!(peer.finish().await.is_err());
            let mut operator_as_dispatcher = database::Worker::spawn("replay-dispatch",
                db.replay_options(true, false).to_url_lossy().as_str(), &db.consumer, "research", &url).await?;
            operator_as_dispatcher.begin(b'c', &[]).await?;
            assert!(operator_as_dispatcher.finish().await.is_err());

            let mut unsupported: serde_json::Value = serde_json::from_slice(bytes)?;
            unsupported["type"] = "com.edgeagent.conformance.apply-probe.v9".into();
            for (key, route, payload, subscriber) in [
                ("malformed", subject.clone(), b"not-json".to_vec(), consumer.clone()),
                ("unsupported", subject.clone(), serde_json::to_vec(&unsupported)?, consumer.clone()),
                ("misrouted", "arbitrary.subject".into(), bytes.to_vec(), consumer.clone()),
                ("foreign-subscriber", subject.clone(), bytes.to_vec(), "foreign_subscriber".into()),
            ] {
                retain(db, &subscriber, key, &route, &payload).await?;
                assert!(authorize(db, &url, true, "research", &inbound(key, &subscriber, key), true).await.is_err());
            }
            assert_eq!(db.count(true, "edgeagent_message_quarantine_replay_audit").await?, 1);
            assert_eq!(db.count(true, "conformance_replay_dispatch").await?, 1);
            let attempts: i32 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT attempt_count FROM {}.conformance_replay_dispatch", db.consumer))).fetch_one(&mut db.admin).await?;
            assert_eq!(attempts, 0, "unauthorized dispatch never claims work");
            permissions(db).await?;
            Ok(())
        })).await
    }).await
}

async fn permissions(db: &database::Database) -> TestResult {
    for (options, statements) in [
        (
            db.runtime_options(true, false),
            vec![
                format!(
                    "INSERT INTO {}.conformance_replay_dispatch (request_id) SELECT 'x' WHERE FALSE",
                    db.consumer
                ),
                format!(
                    "INSERT INTO {}.edgeagent_message_quarantine_replay_audit (replay_request_id) SELECT 'x' WHERE FALSE",
                    db.consumer
                ),
            ],
        ),
        (
            db.replay_options(true, false),
            vec![
                format!(
                    "UPDATE {}.conformance_replay_dispatch SET attempt_count = 3 WHERE FALSE",
                    db.consumer
                ),
                format!(
                    "UPDATE {}.edgeagent_message_quarantine SET payload = 'changed' WHERE FALSE",
                    db.consumer
                ),
                format!(
                    "UPDATE {}.edgeagent_message_quarantine_replay_audit SET reason = 'changed' WHERE FALSE",
                    db.consumer
                ),
                format!("SELECT * FROM {}.conformance_acceptance", db.producer),
            ],
        ),
        (
            db.replay_options(true, true),
            vec![
                format!(
                    "UPDATE {}.conformance_replay_dispatch SET payload = 'changed' WHERE FALSE",
                    db.consumer
                ),
                format!(
                    "INSERT INTO {}.edgeagent_message_quarantine_replay_audit (replay_request_id) SELECT 'x' WHERE FALSE",
                    db.consumer
                ),
                format!(
                    "UPDATE {}.conformance_replay_receipt SET duplicate = FALSE WHERE FALSE",
                    db.consumer
                ),
            ],
        ),
    ] {
        let mut connection = sqlx::PgConnection::connect_with(&options).await?;
        for statement in statements {
            let error = sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                .execute(&mut connection)
                .await
                .err()
                .ok_or("unexpected control-plane privilege")?;
            assert_eq!(
                error.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("42501")
            );
        }
        connection.close().await?;
    }
    Ok(())
}
