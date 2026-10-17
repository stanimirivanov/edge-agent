use super::{broker, database, plan, scenarios::*};
use broker::{TestError, TestResult};
use edgeagent_contracts::Component;
use edgeagent_event_spine_conformance::{definitions, registry};
use edgeagent_messaging_nats::topology::{Binding, TopologyLimits, TopologyPlan};
use sqlx::Connection;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn competing_replicas_respect_shared_inflight_high_water() -> TestResult {
    let topology = TopologyPlan::new(
        registry()?,
        "m02-process",
        &[Binding::new(
            Component::Gateway,
            definitions()[0].message_type(),
        )],
        &[Binding::new(
            Component::AuditProjector,
            definitions()[1].message_type(),
        )],
        TopologyLimits::new(100, 1024 * 1024, 2, 4)?,
    )?;
    let provisioned = topology.clone();
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &provisioned).await?;
        database::with_database(|db| {
            Box::pin(async move {
                for probe in ["one", "two", "three"] {
                    command(db, &url, probe, probe, 0, 42).await?;
                }
                let first = held(db, &url).await?;
                let second = held(db, &url).await?;
                let mut third = db.spawn("consumer", true, &url).await?;
                third.begin(b'h', &[]).await?;
                let mut consumer = durable(&url).await?;
                // A confirmed waiting pull establishes the third replica is active;
                // no negative assertion depends on sleeping for a guessed interval.
                tokio::time::timeout(Duration::from_secs(5), async {
                    let mut poll = tokio::time::interval(Duration::from_millis(10));
                    loop {
                        poll.tick().await;
                        let info = consumer.info().await?;
                        assert_eq!(info.num_ack_pending, 2);
                        assert_eq!(info.num_pending, 1);
                        if info.num_waiting == 1 {
                            return Ok::<(), TestError>(());
                        }
                    }
                })
                .await??;
                assert_eq!(db.count(true, "conformance_probe").await?, 0);
                assert_eq!(release(first).await?, "applied");
                third.expect("delivered").await?;
                assert_eq!(consumer.info().await?.num_ack_pending, 2);
                assert_eq!(release(second).await?, "applied");
                assert_eq!(release(third).await?, "applied");
                let info = consumer.info().await?;
                assert_eq!((info.num_ack_pending, info.num_pending), (0, 0));
                for table in [
                    "conformance_probe",
                    "conformance_command",
                    "edgeagent_message_inbox",
                    "edgeagent_message_outbox",
                ] {
                    assert_eq!(db.count(true, table).await?, 3);
                }
                Ok(())
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn locked_partition_and_connection_loss_preserve_other_progress_and_recovery() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        provision_broker(&url, &plan()?).await?;
        database::with_database(|db| Box::pin(async move {
            command(db, &url, "hot-initial", "hot", 0, 42).await?;
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            command(db, &url, "hot-next", "hot", 1, 43).await?;
            command(db, &url, "cold-initial", "cold", 0, 7).await?;
            let mut hot = held(db, &url).await?;
            let cold = held(db, &url).await?;
            let application = hot.application_name()?;
            let mut inspector = db.inspector().await?;
            let mut blocker = inspector.begin().await?;
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT revision FROM {}.conformance_probe WHERE probe_id = 'hot' FOR UPDATE", db.consumer
            ))).execute(&mut *blocker).await?;
            hot.begin(b'c', &[]).await?;
            wait_for_lock(&mut db.admin, &application).await?;
            assert_eq!(release(cold).await?, "applied", "unrelated partition must progress before lock release");
            assert_eq!(state(db, "cold").await?, Some((1, 7)));
            assert_eq!(state(db, "hot").await?, Some((1, 42)));
            // Terminate precisely this owned runtime connection during the open
            // inbox transaction, never the administrative or blocking session.
            let terminated: Vec<bool> = sqlx::query_scalar(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1 AND usename = $2 AND wait_event_type = 'Lock'"
            ).bind(&application).bind(&db.consumer).fetch_all(&mut db.admin).await?;
            assert_eq!(terminated, [true]);
            assert_eq!(hot.finish().await?, "retry");
            blocker.rollback().await?;
            inspector.close().await?;
            assert_eq!(db.count(true, "conformance_command").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 2);
            assert_eq!(db.count(true, "edgeagent_message_quarantine").await?, 0);
            assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
            assert_eq!(state(db, "hot").await?, Some((2, 43)));
            for table in ["conformance_command", "edgeagent_message_inbox", "edgeagent_message_outbox"] {
                assert_eq!(db.count(true, table).await?, 3);
            }
            Ok(())
        })).await
    }).await
}
