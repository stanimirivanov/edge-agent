//! Real independent OS-process composition; no adapter crash hooks.

#[path = "topology/fixture.rs"]
mod broker;
#[path = "process/fixture.rs"]
mod database;
#[path = "process/replay/mod.rs"]
mod replay;
#[path = "process/replicas.rs"]
mod replicas;
#[path = "process/scenarios.rs"]
mod scenarios;
#[path = "process/semantics.rs"]
mod semantics;

use broker::{TestError, TestResult};
use edgeagent_contracts::{Component, MessageEnvelope};
use edgeagent_event_spine_conformance::{AcceptanceMessage, definitions, registry};
use edgeagent_messaging::{DeliveryDisposition, MessageConsumer};
use edgeagent_messaging_nats::{
    JetStreamConsumer,
    topology::{Binding, TopologyLimits, TopologyPlan, provision},
};
use sqlx::Connection;
use std::time::Duration;

fn plan() -> Result<TopologyPlan, TestError> {
    Ok(TopologyPlan::new(
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
        TopologyLimits::default(),
    )?)
}

#[test]
fn process_broker_image_matches_local_profile() {
    assert!(broker::pinned_image_matches_local_profile());
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn independent_processes_commit_acceptance_transition_and_fact_once() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        let topology = plan()?;
        provision(
            &broker::context(&url, topology.provisioner_permissions()).await?,
            &topology,
        )
        .await?;
        database::with_database(|db| {
            Box::pin(async move {
                assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
                // Re-enter a new producer process with the same immutable identity.
                assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
                assert_eq!(db.count(false, "conformance_acceptance").await?, 1);
                assert_eq!(db.count(false, "edgeagent_message_outbox").await?, 1);
                assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 0);
                assert_eq!(db.worker("relay", false, &url, true).await?, "published");
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                assert_counts(db).await?;
                assert_eq!(db.worker("relay", true, &url, true).await?, "published");
                assert_eq!(db.worker("relay", false, &url, true).await?, "idle");
                assert_eq!(db.worker("relay", true, &url, true).await?, "idle");
                for schema in [&db.producer, &db.consumer] {
                    let published: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                        "SELECT published_at IS NOT NULL FROM {schema}.edgeagent_message_outbox"
                    )))
                    .fetch_one(&mut db.admin)
                    .await?;
                    assert!(
                        published,
                        "publication must remain durable after relay exit"
                    );
                }
                let inspector = broker::connect(&url, "inspector", "_INBOX.inspector").await?;
                let context = async_nats::jetstream::new(inspector);
                let command_stream = topology
                    .streams()
                    .iter()
                    .find(|s| s.definition() == definitions()[0])
                    .ok_or("command stream absent")?;
                let mut stream = context.get_stream(&command_stream.config().name).await?;
                assert_eq!(
                    stream.info().await?.state.messages,
                    0,
                    "ACK removed command after commit"
                );
                let command_durable = command_stream
                    .consumers()
                    .first()
                    .and_then(|consumer| consumer.config().durable_name.as_deref())
                    .ok_or("command durable absent")?;
                let mut durable = stream
                    .get_consumer::<async_nats::jetstream::consumer::pull::Config>(command_durable)
                    .await?;
                assert_eq!(durable.info().await?.num_ack_pending, 0);

                // Send a second transport publication without Nats-Msg-Id. Inbox
                // identity, not the broker's dedup window, must suppress the effect.
                let bytes: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox",
                    db.producer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                context
                    .publish(definitions()[0].subject()?, bytes.clone().into())
                    .await?
                    .await?;
                assert_eq!(db.worker("consumer", true, &url, true).await?, "duplicate");
                assert_counts(db).await?;
                assert_eq!(stream.info().await?.state.messages, 0);
                assert_eq!(durable.info().await?.num_ack_pending, 0);
                let fact_stream = topology
                    .streams()
                    .iter()
                    .find(|s| s.definition() == definitions()[1])
                    .ok_or("fact stream absent")?;
                let mut facts = context.get_stream(&fact_stream.config().name).await?;
                assert_eq!(facts.info().await?.state.messages, 1);
                let role = topology
                    .runtime_permissions()
                    .iter()
                    .find(|role| role.component() == Some(Component::AuditProjector))
                    .ok_or("audit role absent")?;
                let audit = broker::context(&url, role).await?;
                let fact_durable = fact_stream
                    .consumers()
                    .iter()
                    .find(|consumer| consumer.component() == Component::AuditProjector)
                    .and_then(|consumer| consumer.config().durable_name.as_deref())
                    .ok_or("fact durable absent")?;
                let durable = audit
                    .get_stream(&fact_stream.config().name)
                    .await?
                    .get_consumer(fact_durable)
                    .await?;
                let mut consumer = JetStreamConsumer::new(durable).await?;
                let fact =
                    tokio::time::timeout(Duration::from_secs(5), consumer.receive()).await??;
                let envelope = MessageEnvelope::from_json(fact.payload())?;
                let AcceptanceMessage::Fact(payload) = AcceptanceMessage::from_envelope(&envelope)?
                else {
                    return Err("expected typed fact".into());
                };
                assert_eq!(
                    (payload.probe_id(), payload.revision(), payload.value()),
                    ("probe-01", 1, 42)
                );
                let command = MessageEnvelope::from_json(&bytes)?;
                assert_eq!(envelope.extension("causationid"), Some(command.id()));
                assert_eq!(
                    envelope.extension("correlationid"),
                    command.extension("correlationid")
                );
                fact.settle(DeliveryDisposition::Acknowledge).await?;
                assert_permissions(db).await?;
                Ok(())
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn process_rollbacks_leave_no_partial_acceptance_or_inbound_fact() -> TestResult {
    let topology = plan()?;
    broker::with_broker(&topology, |url| async move {
        let topology = plan()?;
        provision(
            &broker::context(&url, topology.provisioner_permissions()).await?,
            &topology,
        )
        .await?;
        database::with_database(|db| {
            Box::pin(async move {
                assert_eq!(
                    db.worker("producer", false, &url, false).await?,
                    "rolled_back"
                );
                assert_eq!(db.count(false, "conformance_acceptance").await?, 0);
                assert_eq!(db.count(false, "edgeagent_message_outbox").await?, 0);
                assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
                assert_eq!(db.worker("relay", false, &url, true).await?, "published");
                assert_eq!(
                    db.worker("consumer", true, &url, false).await?,
                    "rolled_back"
                );
                for table in [
                    "edgeagent_message_inbox",
                    "conformance_probe",
                    "conformance_command",
                    "edgeagent_message_outbox",
                ] {
                    assert_eq!(
                        db.count(true, table).await?,
                        0,
                        "inbound rollback must remove {table}"
                    );
                }
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                assert_counts(db).await?;
                assert_eq!(db.worker("relay", true, &url, true).await?, "published");
                Ok(())
            })
        })
        .await
    })
    .await
}

async fn assert_counts(db: &mut database::Database) -> TestResult {
    for table in [
        "edgeagent_message_inbox",
        "conformance_probe",
        "conformance_command",
        "edgeagent_message_outbox",
    ] {
        assert_eq!(db.count(true, table).await?, 1, "one durable {table}");
    }
    let (revision, value): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT revision, value FROM {}.conformance_probe",
        db.consumer
    )))
    .fetch_one(&mut db.admin)
    .await?;
    assert_eq!((revision, value), (1, 42));
    Ok(())
}

async fn assert_permissions(db: &database::Database) -> TestResult {
    for consumer in [false, true] {
        let peer = if consumer { &db.producer } else { &db.consumer };
        let own = if consumer { &db.consumer } else { &db.producer };
        for relay in [false, true] {
            let mut connection =
                sqlx::PgConnection::connect_with(&db.runtime_options(consumer, relay)).await?;
            let mut denied = vec![
                format!("INSERT INTO {peer}.edgeagent_message_outbox DEFAULT VALUES"),
                format!("CREATE TABLE {own}.forbidden (id INTEGER)"),
            ];
            if relay {
                denied.push(format!(
                    "INSERT INTO {own}.edgeagent_message_outbox DEFAULT VALUES"
                ));
                let table = if consumer {
                    "conformance_probe"
                } else {
                    "conformance_acceptance"
                };
                denied.push(format!("INSERT INTO {own}.{table} DEFAULT VALUES"));
            } else {
                denied.push(format!(
                    "UPDATE {own}.edgeagent_message_outbox SET attempt_count = 99 WHERE FALSE"
                ));
            }
            if consumer {
                denied.push(format!(
                    "INSERT INTO {own}.edgeagent_message_quarantine_replay_audit DEFAULT VALUES"
                ));
                denied.push(format!(
                    "UPDATE {own}.conformance_command SET value = 99 WHERE FALSE"
                ));
                denied.push(format!("DELETE FROM {own}.conformance_command WHERE FALSE"));
            }
            for sql in denied {
                let error = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(&mut connection)
                    .await
                    .err()
                    .ok_or(
                        "cross-service write/DDL or excess runtime grant unexpectedly permitted",
                    )?;
                assert_eq!(
                    error.as_database_error().and_then(|e| e.code()).as_deref(),
                    Some("42501")
                );
            }
            connection.close().await?;
        }
    }
    Ok(())
}
