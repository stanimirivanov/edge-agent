//! Real projector consumer ACK/restart, route refusal and unresolved-owner gap semantics.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned isolated schema inspection."
)]
#[allow(
    dead_code,
    reason = "Shared fixture also provides persistence crash/fault helpers."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use edgeagent_contracts::research::ResearchRecord;
use edgeagent_research_application::publication::{ABSTAINED, PUBLISHED};
use futures_util::FutureExt;
use std::{panic::AssertUnwindSafe, time::Duration};
use support::*;
async fn worker(db: &Database, broker: &str, name: &str) -> Result {
    let mut command = db.command("offline-worker");
    command
        .env("EDGEAGENT_NATS_URL", broker)
        .env("EDGEAGENT_PROJECTOR_STREAM", name)
        .env("EDGEAGENT_PROJECTOR_CONSUMER", "projector")
        .env("EDGEAGENT_PROJECTOR_MAX_MESSAGES", "1");
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(40), command.output()).await??;
    if !output.status.success() {
        return Err(format!(
            "projector process failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL and NATS via EDGEAGENT_POSTGRES_URL and EDGEAGENT_NATS_URL"]
async fn broker_restart_duplicates_route_poison_and_conflicts_preserve_applied_progress() -> Result
{
    let db = Database::new().await?;
    let broker = std::env::var("EDGEAGENT_NATS_URL")?;
    edgeagent_audit_projector::runtime::local(&broker, &["nats", "tls"])?;
    let client = async_nats::connect(&broker).await?;
    let context = async_nats::jetstream::new(client.clone());
    let name = db.schema.clone();
    let result = AssertUnwindSafe(async {
        let a = PUBLISHED.subject()?;
        let b = ABSTAINED.subject()?;
        let stream = context
            .create_stream(async_nats::jetstream::stream::Config {
                name: name.clone(),
                subjects: vec![
                    a.clone(),
                    b.clone(),
                    edgeagent_research_application::lifecycle::CHANGED.subject()?,
                ],
                max_messages: 32,
                max_bytes: 16_777_216,
                max_age: Duration::from_secs(600),
                max_message_size: 262_144,
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await?;
        let mut consumer = stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("projector".into()),
                filter_subjects: vec![
                    a.clone(),
                    b.clone(),
                    edgeagent_research_application::lifecycle::CHANGED.subject()?,
                ],
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(60),
                max_deliver: 3,
                max_ack_pending: 8,
                ..Default::default()
            })
            .await?;
        let record = record("request-1", false)?;
        let envelope = event(&record)?;
        let store = db.store()?;
        context
            .publish(a.clone(), envelope.to_json()?.into())
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        let first = store.get("offline", "request-1").await?.ok_or("view")?;
        assert_eq!(first.status, "artifact");
        context
            .publish(a.clone(), envelope.to_json()?.into())
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            store
                .get("offline", "request-1")
                .await?
                .ok_or("view")?
                .applied_delivery_key,
            first.applied_delivery_key
        );
        assert_eq!(db.counts().await?, (1, 1));
        let abstained = support::record("abstained", true)?;
        context
            .publish(b.clone(), event(&abstained)?.to_json()?.into())
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(
            store
                .get("offline", "abstained")
                .await?
                .ok_or("view")?
                .status,
            "abstained"
        );
        let closed = support::record("closed", false)?;
        context
            .publish(a.clone(), event(&closed)?.to_json()?.into())
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        let life = edgeagent_contracts::research::ResearchLifecycle {
            scope: "offline".into(),
            request_id: "closed".into(),
            artifact: closed.digest()?,
            revision: 1,
            effective_at_ms: 200,
            change: edgeagent_contracts::research::LifecycleChange::Expired,
        };
        context
            .publish(
                edgeagent_research_application::lifecycle::CHANGED.subject()?,
                edgeagent_research_application::lifecycle::publication(
                    &life,
                    "2026-10-29T00:00:00Z",
                )?
                .to_json()?
                .into(),
            )
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            store
                .get("offline", "closed")
                .await?
                .ok_or("closed")?
                .status,
            "expired"
        );
        let poison = event(&support::record("wrong-route", false)?)?;
        context.publish(b, poison.to_json()?.into()).await?.await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine")
                .fetch_one(&db.pool)
                .await?,
            1
        );
        assert_eq!(db.counts().await?, (3, 4));
        let mut changed = record;
        if let ResearchRecord::Artifact(v) = &mut changed {
            v.expires_at_ms += 1;
        }
        context
            .publish(a, event(&changed)?.to_json()?.into())
            .await?
            .await?;
        worker(&db, &broker, &name).await?;
        assert_eq!(consumer.info().await?.num_ack_pending, 1);
        let blocked = store.get("offline", "request-1").await?.ok_or("gap")?;
        assert_eq!(blocked.status, "gap");
        assert_eq!(blocked.applied_delivery_key, first.applied_delivery_key);
        assert_eq!(db.counts().await?, (3, 4));
        Ok(())
    })
    .catch_unwind()
    .await;
    let _ = context.delete_stream(&name).await;
    client.drain().await?;
    db.finish(result).await
}
