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
// Exercise the operator entry point without installing or changing approval.
async fn recover(
    profile: &Profile,
    schema: &str,
    bytes: &[u8],
    scope: &str,
) -> Result<std::process::Output> {
    use tokio::io::AsyncWriteExt as _;
    let mut command = profile.command(schema, "offline-once");
    command.env("EDGEAGENT_RESEARCH_SCOPE", scope);
    let mut child = tokio::process::Command::from(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    child.stdin.take().ok_or("stdin")?.write_all(bytes).await?;
    Ok(tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await??)
}
async fn recovery_state(db: &Database) -> Result<(i64, i64, i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT (SELECT count(*) FROM edgeagent_research_completions),
         (SELECT count(*) FROM edgeagent_research_decisions),
         (SELECT count(*) FROM edgeagent_message_inbox),
         (SELECT count(*) FROM edgeagent_message_outbox)",
    )
    .fetch_one(&db.pool)
    .await?)
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
        prove_explanation_admission(&db, &stream, &context, &broker, &profile, &retained).await?;
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
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine WHERE consumer_name='research-worker'")
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
        let before_delivery = consumer.info().await?.delivered.consumer_sequence;
        let (blocked, owner_bytes) = support::governed_event("disabled")?;
        let blocked_ack = context
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
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine WHERE consumer_name='research-worker'")
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
        let retained = stream.get_raw_message(blocked_ack.sequence).await?;
        assert_eq!(
            MessageEnvelope::from_json(&retained.payload)?.to_json()?,
            blocked.to_json()?
        );
        assert_eq!(
            consumer.info().await?.delivered.consumer_sequence,
            before_delivery + 3
        );
        let recovery_profile = Profile::new(&owner_bytes)?;
        let before = recovery_state(&db).await?;
        let bytes = retained.payload.to_vec();
        assert!(
            !recover(&recovery_profile, &db.schema, &bytes, "operator")
                .await?
                .status
                .success()
        );
        assert_eq!(recovery_state(&db).await?, before);
        // Missing sequence refuses, without falling back to the latest subject message.
        assert!(
            stream
                .get_raw_message(blocked_ack.sequence + 1000)
                .await
                .is_err()
        );
        assert_eq!(recovery_state(&db).await?, before);
        controls.control(&control(4, true)).await?;
        assert!(
            !recover(&recovery_profile, &db.schema, &bytes, "foreign")
                .await?
                .status
                .success()
        );
        assert_eq!(recovery_state(&db).await?, before);
        let recovered = recover(&recovery_profile, &db.schema, &bytes, "operator").await?;
        assert!(
            recovered.status.success(),
            "{}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        assert!(matches!(
            edgeagent_contracts::research::ResearchRecord::decode(&recovered.stdout)?,
            edgeagent_contracts::research::ResearchRecord::Artifact(_)
        ));
        assert_eq!(
            recovery_state(&db).await?,
            (before.0 + 1, before.1 + 1, before.2 + 1, before.3 + 2)
        );
        let inbox: Vec<u8> =
            sqlx::query_scalar("SELECT envelope FROM edgeagent_message_inbox WHERE message_id=$1")
                .bind(blocked.id())
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(inbox, bytes);
        let after = recovery_state(&db).await?;
        recovery_profile.unavailable()?;
        let retry = recover(&recovery_profile, &db.schema, &bytes, "operator").await?;
        assert!(retry.status.success());
        assert_eq!(retry.stdout, recovered.stdout);
        assert_eq!(recovery_state(&db).await?, after);
        assert_eq!(
            consumer.info().await?.delivered.consumer_sequence,
            before_delivery + 3
        );
        assert_eq!(
            stream
                .get_raw_message(blocked_ack.sequence)
                .await?
                .payload
                .as_ref(),
            bytes
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    let _ = context.delete_stream(&name).await;
    client.drain().await?;
    db.finish(result).await
}

async fn prove_explanation_admission(
    db: &Database,
    stream: &async_nats::jetstream::stream::Stream,
    context: &async_nats::jetstream::Context,
    broker: &str,
    profile: &Profile,
    retained: &[u8],
) -> Result {
    let subject = PUBLISHED.subject()?;
    let mut consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("explanations".into()),
            filter_subject: subject.clone(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ack_wait: Duration::from_secs(60),
            max_deliver: 3,
            max_ack_pending: 1,
            ..Default::default()
        })
        .await?;
    let run = |scope: &str| {
        let mut command = profile.command(&db.schema, "offline-explanation-scheduler");
        command
            .env_remove("EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION")
            .env("EDGEAGENT_NATS_URL", broker)
            .env("EDGEAGENT_RESEARCH_STREAM", &db.schema)
            .env("EDGEAGENT_RESEARCH_CONSUMER", "explanations")
            .env("EDGEAGENT_RESEARCH_MAX_MESSAGES", "1")
            .env("EDGEAGENT_RESEARCH_SCOPE", scope);
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        async move {
            let output = tokio::time::timeout(Duration::from_secs(40), command.output()).await??;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
    };
    let inbox_count = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM edgeagent_message_inbox WHERE consumer_name='research-explanation-jobs'",
        )
        .fetch_one(&db.pool)
        .await
    };

    // Authenticated bytes on the correct subject still cannot cross the configured scope.
    run("foreign").await?;
    assert_eq!(inbox_count().await?, 0);

    let mut forged: serde_json::Value = serde_json::from_slice(retained)?;
    forged["correlationid"] = "forged".into();
    for invalid in [serde_json::to_vec(&forged)?, {
        forged["type"] = ABSTAINED.message_type().into();
        serde_json::to_vec(&forged)?
    }] {
        context
            .publish(subject.clone(), invalid.into())
            .await?
            .await?;
        run("operator").await?;
    }
    assert_eq!(inbox_count().await?, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine WHERE consumer_name='research-explanation-jobs'")
            .fetch_one(&db.pool).await?,
        3
    );

    sqlx::raw_sql(
        "CREATE FUNCTION fail_explanation_commit() RETURNS trigger LANGUAGE plpgsql AS
         $$ BEGIN RAISE EXCEPTION 'proof commit failure'; END $$;
         CREATE CONSTRAINT TRIGGER fail_explanation_commit AFTER INSERT ON edgeagent_research_explanation_jobs
         DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_explanation_commit();",
    )
    .execute(&db.pool)
    .await?;
    context
        .publish(subject.clone(), retained.to_vec().into())
        .await?
        .await?;
    run("operator").await?;
    assert_eq!(inbox_count().await?, 0);
    let jobs = || async {
        sqlx::query_as::<_, (String, Vec<u8>)>(
            "SELECT job_id, job FROM edgeagent_research_explanation_jobs ORDER BY job_id",
        )
        .fetch_all(&db.pool)
        .await
    };
    assert!(jobs().await?.is_empty());
    assert_eq!(consumer.info().await?.num_ack_pending, 1);
    sqlx::raw_sql("DROP TRIGGER fail_explanation_commit ON edgeagent_research_explanation_jobs")
        .execute(&db.pool)
        .await?;
    let mut pending = consumer
        .fetch()
        .max_messages(1)
        .expires(Duration::from_secs(5))
        .messages()
        .await?;
    let delivery = pending
        .next()
        .await
        .ok_or("missing retried publication")??;
    let sequence = delivery.info()?.stream_sequence;
    let store = edgeagent_research_postgres::PostgresResearchStore::new(db.pool.clone());
    store
        .admit_explanation_job("operator", &MessageEnvelope::from_json(&delivery.payload)?)
        .await?;
    // Simulate losing the successful ACK, then ask JetStream to redeliver without waiting for AckWait.
    delivery
        .ack_with(async_nats::jetstream::AckKind::Nak(None))
        .await?;
    drop(pending);
    run("operator").await?;
    assert_eq!(consumer.info().await?.ack_floor.stream_sequence, sequence);
    let admitted = jobs().await?;
    assert_eq!(admitted.len(), 1);
    assert_eq!(inbox_count().await?, 1);
    let job = edgeagent_contracts::explanation::Job::decode(&admitted[0].1)?;
    assert_eq!(job.id()?, admitted[0].0);
    let artifact: edgeagent_contracts::research::ResearchRecord =
        MessageEnvelope::from_json(retained)?.payload()?;
    let decision: Vec<u8> =
        sqlx::query_scalar("SELECT publication FROM edgeagent_research_decisions")
            .fetch_one(&db.pool)
            .await?;
    job.bind(
        &artifact,
        &edgeagent_research_application::publication::decode_decision(
            &MessageEnvelope::from_json(&decision)?,
        )?,
    )?;
    context
        .publish(subject, retained.to_vec().into())
        .await?
        .await?;
    run("operator").await?;
    assert_eq!(consumer.info().await?.num_ack_pending, 0);
    assert_eq!(jobs().await?, admitted);
    assert_eq!(inbox_count().await?, 1);
    Ok(())
}
