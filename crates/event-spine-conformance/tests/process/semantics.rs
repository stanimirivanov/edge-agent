use super::{broker, database, plan, scenarios::*};
use broker::TestResult;
use edgeagent_contracts::MessageEnvelope;
use edgeagent_event_spine_conformance::AcceptanceMessage;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn competing_semantic_commands_commit_one_transition_and_retain_conflicts() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| {
            Box::pin(async move {
                command(db, &url, "same-a", "same", 0, 42).await?;
                command(db, &url, "same-b", "same", 0, 42).await?;
                let first = held(db, &url).await?;
                let second = held(db, &url).await?;
                // Both independent replicas own deliveries before either can commit.
                let (a, b) = tokio::try_join!(release(first), release(second))?;
                assert_eq!((a.as_str(), b.as_str()), ("applied", "applied"));
                assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 2);
                assert_eq!(db.count(true, "conformance_command").await?, 1);
                assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 1);
                assert_eq!(state(db, "same").await?, Some((1, 42)));
                let fact_before: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox",
                    db.consumer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                let fact = MessageEnvelope::from_json(&fact_before)?;
                assert!(matches!(
                    fact.extension("causationid"),
                    Some("same-a" | "same-b")
                ));
                let AcceptanceMessage::Fact(payload) = AcceptanceMessage::from_envelope(&fact)?
                else {
                    return Err("fact required".into());
                };
                assert_eq!(
                    (payload.probe_id(), payload.revision(), payload.value()),
                    ("same", 1, 42)
                );

                command(db, &url, "changed", "same", 0, 99).await?;
                assert_eq!(
                    db.worker("consumer", true, &url, true).await?,
                    "quarantined:semantic_conflict"
                );
                let evidence = quarantine(db).await?;
                assert_eq!(evidence.len(), 1);
                assert_eq!((&*evidence[0].0, evidence[0].1), ("semantic_conflict", 1));
                let rejected = MessageEnvelope::from_json(&evidence[0].2)?;
                assert_eq!(rejected.id(), "changed");
                let original: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox WHERE message_id = 'changed'",
                    db.producer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                assert_eq!(
                    evidence[0].2, original,
                    "quarantine retains the exact transport bytes"
                );
                let fact_after: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox",
                    db.consumer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                assert_eq!(fact_after, fact_before);
                assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 2);
                assert_eq!(db.count(true, "conformance_command").await?, 1);
                assert_eq!(state(db, "same").await?, Some((1, 42)));
                let accepted: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT value FROM {}.conformance_command WHERE probe_id = 'same' AND expected_revision = 0",
                    db.consumer
                ))).fetch_one(&mut db.admin).await?;
                assert_eq!(accepted, 42, "conflict must not rewrite accepted content");
                Ok(())
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn missing_predecessors_recover_in_order_or_exhaust_into_quarantine() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| {
            Box::pin(async move {
                command(db, &url, "later", "ordered", 1, 43).await?;
                assert_eq!(db.worker("consumer", true, &url, true).await?, "deferred");
                for table in [
                    "edgeagent_message_inbox",
                    "conformance_probe",
                    "conformance_command",
                    "edgeagent_message_outbox",
                ] {
                    assert_eq!(
                        db.count(true, table).await?,
                        0,
                        "gap must roll back {table}"
                    );
                }
                // Hold the known redelivery so the next process receives the newly
                // published predecessor, independent of JetStream retry scheduling.
                let later = held(db, &url).await?;
                command(db, &url, "earlier", "ordered", 0, 42).await?;
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                assert_eq!(state(db, "ordered").await?, Some((1, 42)));
                assert_eq!(release(later).await?, "applied");
                assert_eq!(state(db, "ordered").await?, Some((2, 43)));
                // History, not the latest value, decides an old semantic duplicate.
                command(db, &url, "old-copy", "ordered", 0, 42).await?;
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                assert_eq!(state(db, "ordered").await?, Some((2, 43)));
                assert_eq!(db.count(true, "conformance_command").await?, 2);
                assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 2);
                assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 3);

                let facts: Vec<Vec<u8>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox ORDER BY message_id",
                    db.consumer
                )))
                .fetch_all(&mut db.admin)
                .await?;
                for (bytes, (revision, value, cause)) in
                    facts.iter().zip([(1, 42, "earlier"), (2, 43, "later")])
                {
                    let fact = MessageEnvelope::from_json(bytes)?;
                    let AcceptanceMessage::Fact(payload) = AcceptanceMessage::from_envelope(&fact)?
                    else {
                        return Err("fact required".into());
                    };
                    assert_eq!(
                        (payload.probe_id(), payload.revision(), payload.value()),
                        ("ordered", revision, value)
                    );
                    assert_eq!(fact.extension("causationid"), Some(cause));
                    assert_eq!(fact.extension("correlationid"), Some("m02-ordering"));
                }

                command(db, &url, "never-ready", "absent", 1, 1).await?;
                for attempt in 1..=5 {
                    let expected = if attempt < 5 {
                        "deferred"
                    } else {
                        "quarantined:missing_predecessor"
                    };
                    assert_eq!(db.worker("consumer", true, &url, true).await?, expected);
                    assert_eq!(state(db, "absent").await?, None);
                    assert_eq!(db.count(true, "conformance_command").await?, 2);
                    assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 3);
                    assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 2);
                }
                let evidence = quarantine(db).await?;
                assert_eq!(evidence.len(), 1);
                assert_eq!((&*evidence[0].0, evidence[0].1), ("missing_predecessor", 5));
                assert_eq!(
                    MessageEnvelope::from_json(&evidence[0].2)?.id(),
                    "never-ready"
                );
                let mut consumer = durable(&url).await?;
                let info = consumer.info().await?;
                assert_eq!((info.num_ack_pending, info.num_pending), (0, 0));
                Ok(())
            })
        })
        .await
    })
    .await
}
