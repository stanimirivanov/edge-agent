//! M02 fault qualification uses real processes, durable stores and wire loss.

mod inputs;
mod proxy;

use super::{assert_counts, broker, database, definitions, plan, scenarios};
use broker::{TestError, TestResult};
use std::time::Duration;

async fn prepare(url: &str) -> TestResult {
    let topology = plan()?;
    scenarios::provision_broker(url, &topology).await?;
    let context =
        async_nats::jetstream::new(broker::connect(url, "inspector", "_INBOX.inspector").await?);
    let stream = topology
        .streams()
        .iter()
        .find(|s| s.definition() == definitions()[0])
        .ok_or("command stream absent")?;
    let mut consumer = stream
        .consumers()
        .first()
        .ok_or("command consumer absent")?
        .config()
        .clone();
    // Accelerate only this disposable fixture's explicit ACK deadline. This
    // does not change the production topology or rely on guessed progress.
    consumer.ack_wait = Duration::from_millis(200);
    context
        .get_stream(&stream.config().name)
        .await?
        .update_consumer(consumer)
        .await?;
    Ok(())
}

async fn accepted(db: &database::Database, url: &str) -> TestResult {
    assert_eq!(db.worker("producer", false, url, true).await?, "accepted");
    assert_eq!(db.worker("relay", false, url, true).await?, "published");
    Ok(())
}

async fn redelivery(db: &database::Database, url: &str, expected: &str) -> TestResult {
    let before = scenarios::durable(url).await?.info().await?.clone();
    let mut worker = db.spawn("consumer", true, url).await?;
    worker.begin(b'v', &[]).await?;
    let report = worker.line().await?;
    let (attempt, key) = report
        .strip_prefix("delivery:")
        .ok_or("delivery report absent")?
        .split_once(':')
        .ok_or("delivery key absent")?;
    let attempt: u32 = attempt.parse()?;
    assert_eq!(
        key,
        format!(
            "{}:{}:{}",
            before.stream_name.len(),
            before.stream_name,
            before.delivered.stream_sequence
        )
    );
    assert!(
        attempt >= 2,
        "actual per-delivery metadata must prove redelivery"
    );
    worker.begin(b'c', &[]).await?;
    assert_eq!(worker.finish().await?, expected);
    Ok(())
}

async fn pending(url: &str, expected: usize) -> TestResult {
    let info = scenarios::durable(url).await?.info().await?.clone();
    assert_eq!((info.num_ack_pending, info.num_pending), (expected, 0));
    Ok(())
}

async fn outbox(db: &mut database::Database) -> Result<(i64, bool, Vec<u8>), TestError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT attempt_count::bigint, published_at IS NOT NULL, envelope FROM {}.edgeagent_message_outbox", db.producer
    ))).fetch_one(&mut db.admin).await?)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn killed_producers_recover_before_commit_and_after_durable_enqueue() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        prepare(&url).await?;
        database::with_database(|db| {
            Box::pin(async move {
                let mut before = db.spawn("producer", false, &url).await?;
                before.begin(b'b', &[]).await?;
                before.expect("before_commit").await?;
                assert_eq!(db.count(false, "conformance_acceptance").await?, 0);
                assert_eq!(db.count(false, "edgeagent_message_outbox").await?, 0);
                before.kill().await?;
                let mut after = db.spawn("producer", false, &url).await?;
                after.begin(b'e', &[]).await?;
                after.expect("enqueued").await?;
                assert_eq!(db.count(false, "conformance_acceptance").await?, 1);
                let original = outbox(db).await?;
                assert_eq!((original.0, original.1), (0, false));
                after.kill().await?;
                // Exact identity/content retry is safe after either lost result.
                assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
                assert_eq!(outbox(db).await?, original);
                assert_eq!(db.worker("relay", false, &url, true).await?, "published");
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                assert_counts(db).await?;
                pending(&url, 0).await
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn killed_relay_recovers_persisted_publication_without_second_effect() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        prepare(&url).await?;
        database::with_database(|db| Box::pin(async move {
            assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
            let mut relay = db.spawn("relay", false, &url).await?;
            relay.begin(b'u', &[]).await?;
            relay.expect("persisted").await?;
            let before = outbox(db).await?;
            assert_eq!((before.0, before.1), (1, false));
            let mut durable = scenarios::durable(&url).await?;
            assert_eq!(durable.info().await?.num_pending, 1);
            relay.kill().await?;
            // Controlled expiry of this one owned lease exercises recovery
            // without waiting ten seconds or changing production lease policy.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {}.edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - interval '1 second' WHERE published_at IS NULL", db.producer
            ))).execute(&mut db.admin).await?;
            assert_eq!(db.worker("relay", false, &url, true).await?, "published");
            let after = outbox(db).await?;
            assert_eq!((after.0, after.1), (2, true));
            assert_eq!(after.2, before.2);
            assert_eq!(durable.info().await?.num_pending, 1, "broker deduplication retains one message");
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            assert_counts(db).await?;
            pending(&url, 0).await
        })).await
    }).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn killed_consumer_after_commit_recovers_as_duplicate_before_ack() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        prepare(&url).await?;
        database::with_database(|db| {
            Box::pin(async move {
                accepted(db, &url).await?;
                let mut worker = db.spawn("consumer", true, &url).await?;
                worker.begin(b'a', &[]).await?;
                worker.expect("committed").await?;
                assert_counts(db).await?;
                pending(&url, 1).await?;
                worker.kill().await?;
                redelivery(db, &url, "duplicate").await?;
                assert_counts(db).await?;
                pending(&url, 0).await
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn killed_file_broker_retains_message_and_durable_across_fresh_workers() -> TestResult {
    broker::with_controlled_broker(&plan()?, |owned| {
        Box::pin(async move {
            let url = owned.lock().await.url().to_owned();
            prepare(&url).await?;
            database::with_database(|db| {
                Box::pin(async move {
                    accepted(db, &url).await?;
                    let worker = scenarios::held(db, &url).await?;
                    let before = scenarios::durable(&url).await?.info().await?.clone();
                    assert_eq!(before.num_ack_pending, 1);
                    worker.kill().await?;
                    owned.lock().await.restart().await?;
                    let url = owned.lock().await.url().to_owned();
                    let after = scenarios::durable(&url).await?.info().await?.clone();
                    assert_eq!(
                        (after.name, after.stream_name, after.created),
                        (before.name, before.stream_name, before.created)
                    );
                    assert_eq!(after.num_ack_pending, 1);
                    redelivery(db, &url, "applied").await?;
                    assert_counts(db).await?;
                    pending(&url, 0).await
                })
            })
            .await
        })
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn wire_ack_request_and_confirmation_loss_have_distinct_recovery() -> TestResult {
    for loss in [proxy::Loss::Request, proxy::Loss::Confirmation] {
        broker::with_broker(&plan()?, |url| async move {
            prepare(&url).await?;
            database::with_database(|db| {
                Box::pin(async move {
                    accepted(db, &url).await?;
                    let proxy = proxy::Proxy::start(&url, loss).await?;
                    assert_eq!(
                        db.worker("consumer", true, &proxy.url, true).await?,
                        "confirmation_unknown"
                    );
                    let evidence = proxy.finish().await?;
                    assert_counts(db).await?;
                    match loss {
                        proxy::Loss::Request => {
                            assert_eq!(
                                evidence,
                                proxy::Evidence {
                                    requests: 1,
                                    forwarded: 0,
                                    confirmations_dropped: 0
                                }
                            );
                            pending(&url, 1).await?;
                            redelivery(db, &url, "duplicate").await?;
                        }
                        proxy::Loss::Confirmation => {
                            assert_eq!(
                                evidence,
                                proxy::Evidence {
                                    requests: 1,
                                    forwarded: 1,
                                    confirmations_dropped: 1
                                }
                            );
                            // A missing confirmation is unknown to the caller, even
                            // though independent inspection proves this ACK arrived.
                            pending(&url, 0).await?;
                        }
                    }
                    assert_counts(db).await?;
                    pending(&url, 0).await
                })
            })
            .await
        })
        .await?;
    }
    Ok(())
}
