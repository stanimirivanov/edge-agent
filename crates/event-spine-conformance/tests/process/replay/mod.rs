mod authorization;
mod outbound;
mod recovery;
mod routes;

use super::{broker, database};
use broker::{TestError, TestResult};
use edgeagent_contracts::Component;
use edgeagent_event_spine_conformance::replay::ReplayCommand;
use edgeagent_inbox_postgres::{PostgresInbox, QuarantineEvidence};
use sqlx::Connection;

fn inbound(request: &str, consumer: &str, key: &str) -> ReplayCommand {
    ReplayCommand::Inbound {
        request_id: request.into(),
        consumer_name: consumer.into(),
        delivery_key: key.into(),
        reason: "dependency_remediated".into(),
    }
}

async fn authorize(
    db: &database::Database,
    url: &str,
    consumer: bool,
    component: &str,
    request: &ReplayCommand,
    commit: bool,
) -> Result<String, TestError> {
    let mut worker = db.replay_worker(consumer, false, component, url).await?;
    worker
        .begin(
            if commit { b'c' } else { b'r' },
            &serde_json::to_vec(request)?,
        )
        .await?;
    worker.finish().await
}

async fn dispatch(
    db: &database::Database,
    url: &str,
    component: &str,
) -> Result<String, TestError> {
    let mut worker = db.replay_worker(true, true, component, url).await?;
    worker.begin(b'c', &[]).await?;
    worker.finish().await
}

async fn retain(
    db: &mut database::Database,
    consumer: &str,
    key: &str,
    subject: &str,
    payload: &[u8],
) -> TestResult {
    let mut tx = db.admin.begin().await?;
    PostgresInbox
        .quarantine_delivery(
            &mut tx,
            consumer,
            QuarantineEvidence::new(key, subject, 1, payload, "fixture_poison")?,
        )
        .await?;
    tx.commit().await?;
    Ok(())
}

fn consumer_name(component: Component) -> String {
    component.as_str().replace('.', "_")
}

async fn expire(db: &mut database::Database) -> TestResult {
    // Fixture clock fault only, never a production lease-reset API. Generation
    // is deliberately untouched so a stale worker cannot finish a newer claim.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {}.conformance_replay_dispatch SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' WHERE lease_owner IS NOT NULL", db.consumer
    ))).execute(&mut db.admin).await?;
    Ok(())
}
