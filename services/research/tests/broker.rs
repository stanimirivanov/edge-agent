//! Real transport settlement/publication; owner process is a deterministic isolated test double.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned isolated schema inspection."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use edgeagent_contracts::{MessageEnvelope, research_acquisition::DECIDED};
use edgeagent_research_application::publication::{ABSTAINED, PUBLISHED};
use futures_util::{FutureExt, StreamExt};
use std::time::Duration;
use support::governed_event as event;
use support::*;
fn control(revision: u64, enabled: bool) -> edgeagent_contracts::research_controls::Command {
    edgeagent_contracts::research_controls::Command::V1 {
        scope: "operator".into(),
        target: edgeagent_contracts::research_controls::Target::Scope,
        revision,
        enabled,
        reason_code: "broker-proof".into(),
    }
}
async fn worker(
    profile: &Profile,
    schema: &str,
    broker: &str,
    stream: &str,
    relay: bool,
) -> Result<std::process::Output> {
    let mut command = profile.command(
        schema,
        if relay {
            "offline-relay"
        } else {
            "offline-worker"
        },
    );
    command
        .env("EDGEAGENT_NATS_URL", broker)
        .env("EDGEAGENT_RESEARCH_STREAM", stream)
        .env("EDGEAGENT_RESEARCH_CONSUMER", "research")
        .env(
            "EDGEAGENT_RESEARCH_MAX_MESSAGES",
            if relay { "16" } else { "1" },
        );
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    Ok(tokio::time::timeout(Duration::from_secs(40), command.output()).await??)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL and NATS via EDGEAGENT_POSTGRES_URL and EDGEAGENT_NATS_URL"]
async fn research_ack_relay_restart_poison_and_retry_exhaustion_preserve_completion() -> Result {
    let db = Database::new().await?;
    let broker = std::env::var("EDGEAGENT_NATS_URL")?;
    edgeagent_research::runtime::local(&broker, &["nats", "tls"])?;
    let client = async_nats::connect(&broker).await?;
    let context = async_nats::jetstream::new(client.clone());
    let name = db.schema.clone();
    let result = std::panic::AssertUnwindSafe(async {
        let (event, bytes) = event("first")?;
        let profile = Profile::new(&bytes)?;
        approve(&db.schema).await?;
        let subject = DECIDED.subject()?;
        let stream = context
            .create_stream(async_nats::jetstream::stream::Config {
                name: name.clone(),
                subjects: vec![
                    subject.clone(),
                    PUBLISHED.subject()?,
                    ABSTAINED.subject()?,
                    edgeagent_research_application::publication::DECISION.subject()?,
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
                durable_name: Some("research".into()),
                filter_subject: subject.clone(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(60),
                max_deliver: 3,
                max_ack_pending: 1,
                ..Default::default()
            })
            .await?;
        context
            .publish(subject.clone(), event.to_json()?.into())
            .await?
            .await?;
        let first = worker(&profile, &db.schema, &broker, &name, false).await?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        let retained: Vec<u8> =
            sqlx::query_scalar("SELECT publication FROM edgeagent_research_completions")
                .fetch_one(&db.pool)
                .await?;
        profile.unavailable()?;
        context
            .publish(subject.clone(), event.to_json()?.into())
            .await?
            .await?;
        assert!(
            worker(&profile, &db.schema, &broker, &name, false)
                .await?
                .status
                .success()
        );
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        let controls = edgeagent_research_postgres::PostgresResearchStore::new(db.pool.clone());
        controls.control(&control(1, false)).await?;
        let mut events = client.subscribe(PUBLISHED.subject()?).await?;
        client.flush().await?;
        assert!(
            worker(&profile, &db.schema, &broker, &name, true)
                .await?
                .status
                .success()
        );
        let message = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await?
            .ok_or("publication")?;
        assert_eq!(
            MessageEnvelope::from_json(&message.payload)?.to_json()?,
            retained
        );
        controls.control(&control(2, true)).await?;
        let (pending, _) = support::governed_event("unavailable")?;
        context
            .publish(subject.clone(), pending.to_json()?.into())
            .await?
            .await?;
        for _ in 0..3 {
            assert!(
                worker(&profile, &db.schema, &broker, &name, false)
                    .await?
                    .status
                    .success()
            );
        }
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        let mut poison: serde_json::Value = serde_json::from_slice(&event.to_json()?)?;
        poison["correlationid"] = "foreign".into();
        context
            .publish(subject, serde_json::to_vec(&poison)?.into())
            .await?
            .await?;
        assert!(
            worker(&profile, &db.schema, &broker, &name, false)
                .await?
                .status
                .success()
        );
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine")
                .fetch_one(&db.pool)
                .await?,
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_completions")
                .fetch_one(&db.pool)
                .await?,
            1
        );
        controls.control(&control(3, false)).await?;
        let (blocked, _) = support::governed_event("disabled")?;
        context
            .publish(DECIDED.subject()?, blocked.to_json()?.into())
            .await?
            .await?;
        for _ in 0..3 {
            assert!(
                worker(&profile, &db.schema, &broker, &name, false)
                    .await?
                    .status
                    .success()
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine")
                .fetch_one(&db.pool)
                .await?,
            2
        );
        assert!(
            !sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM edgeagent_message_inbox WHERE message_id=$1)"
            )
            .bind(blocked.id())
            .fetch_one(&db.pool)
            .await?
        );
        let retained = stream
            .get_last_raw_message_by_subject(&DECIDED.subject()?)
            .await?;
        assert_eq!(
            MessageEnvelope::from_json(&retained.payload)?.to_json()?,
            blocked.to_json()?
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    let _ = context.delete_stream(&name).await;
    client.drain().await?;
    db.finish(result).await
}
