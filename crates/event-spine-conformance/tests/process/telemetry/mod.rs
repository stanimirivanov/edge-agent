mod evidence;
mod receiver;

use super::{broker, database, definitions, plan, scenarios};
use broker::{TestError, TestResult};
use edgeagent_contracts::MessageEnvelope;
use receiver::Receiver;
use std::collections::BTreeSet;

const PARENT: &str = "00-11111111111111111111111111111111-2222222222222222-01";
const STATE: &str = "edgeagent=synthetic,other=bounded";
const PRIVATE_KEY: &str = "private-idempotency-OTLP-sentinel";
const PRIVATE_BAGGAGE: &str = "PRIVATE_BAGGAGE_OTLP_SENTINEL";
const PRIVATE_PAYLOAD: &[u8] = b"PRIVATE_PAYLOAD_OTLP_SENTINEL:not-json";

fn bytes(id: &str) -> Result<Vec<u8>, TestError> {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../fixtures/v1/apply-probe.json"))?;
    value["id"] = id.into();
    value["data"]["probe_id"] = id.into();
    value["subject"] = format!("probe/{id}").into();
    value["partitionkey"] = format!("probe/{id}").into();
    value["correlationid"] = "flow-telemetry".into();
    value["causationid"] = "request-telemetry".into();
    value["idempotencykey"] = PRIVATE_KEY.into();
    value["traceparent"] = PARENT.into();
    value["tracestate"] = STATE.into();
    value["baggage"] = PRIVATE_BAGGAGE.into();
    Ok(serde_json::to_vec(&value)?)
}

async fn observed(
    db: &database::Database,
    url: &str,
    endpoint: &str,
    role: &str,
    consumer: bool,
    action: u8,
    bytes: &[u8],
) -> Result<(String, i64), TestError> {
    let mut worker = db
        .spawn_observed(role, consumer, url, Some(endpoint))
        .await?;
    let name = worker.application_name()?;
    let pid = name
        .strip_prefix("m02_worker_")
        .ok_or("worker identity absent")?
        .parse()?;
    worker.begin(action, bytes).await?;
    Ok((worker.finish().await?, pid))
}

async fn republish(url: &str, bytes: &[u8]) -> TestResult {
    let client = broker::connect(url, "inspector", "_INBOX.inspector").await?;
    async_nats::jetstream::new(client)
        .publish(definitions()[0].subject()?, bytes.to_vec().into())
        .await?
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn exported_process_signals_preserve_context_privacy_and_bounded_dimensions() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        scenarios::provision_broker(&url, &plan()?).await?;
        let owner = std::sync::Arc::new(Receiver::start().await?);
        let receiver = owner.clone();
        let outcome = database::with_database(|db| {
            Box::pin(async move {
                let bytes = bytes("telemetry-good")?;
                let mut pids = BTreeSet::new();
                for (role, consumer, action, input, expected) in [
                    ("producer", false, b'r', bytes.as_slice(), "rolled_back"),
                    ("producer", false, b'c', bytes.as_slice(), "accepted"),
                    ("relay", false, b'c', &[][..], "published"),
                    ("consumer", true, b'r', &[][..], "rolled_back"),
                    ("consumer", true, b'c', &[][..], "applied"),
                    ("relay", true, b'c', &[][..], "published"),
                ] {
                    let (outcome, pid) =
                        observed(db, &url, &receiver.endpoint, role, consumer, action, input)
                            .await?;
                    assert_eq!(outcome, expected);
                    pids.insert(pid);
                    if role == "producer" && action == b'r' {
                        assert_eq!(db.count(false, "conformance_acceptance").await?, 0);
                        assert_eq!(db.count(false, "edgeagent_message_outbox").await?, 0);
                    }
                    if role == "consumer" && action == b'r' {
                        assert_eq!(db.count(true, "edgeagent_message_inbox").await?, 0);
                        assert_eq!(db.count(true, "conformance_probe").await?, 0);
                        assert_eq!(db.count(true, "edgeagent_message_outbox").await?, 0);
                    }
                }
                republish(&url, &bytes).await?;
                let (outcome, pid) =
                    observed(db, &url, &receiver.endpoint, "consumer", true, b'c', &[]).await?;
                assert_eq!(outcome, "duplicate");
                pids.insert(pid);
                republish(&url, PRIVATE_PAYLOAD).await?;
                let (outcome, pid) =
                    observed(db, &url, &receiver.endpoint, "consumer", true, b'c', &[]).await?;
                assert!(outcome.starts_with("quarantined:"));
                pids.insert(pid);
                for table in [
                    "edgeagent_message_inbox",
                    "conformance_command",
                    "conformance_probe",
                    "edgeagent_message_outbox",
                    "edgeagent_message_quarantine",
                ] {
                    assert_eq!(db.count(true, table).await?, 1);
                }
                let fact: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT envelope FROM {}.edgeagent_message_outbox",
                    db.consumer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                let fact = MessageEnvelope::from_json(&fact)?;
                assert_eq!(fact.extension("traceparent"), Some(PARENT));
                assert_eq!(fact.extension("tracestate"), Some(STATE));
                assert_eq!(fact.extension("correlationid"), Some("flow-telemetry"));
                assert_eq!(fact.extension("causationid"), Some("telemetry-good"));
                let durable = scenarios::durable(&url).await?.info().await?.clone();
                assert_eq!(durable.num_ack_pending, 0);
                assert_eq!(durable.num_pending, 0);
                evidence::healthy(receiver.records()?.as_slice(), &pids)?;
                Ok(())
            })
        })
        .await;
        let mut receiver =
            std::sync::Arc::try_unwrap(owner).map_err(|_| "receiver still borrowed")?;
        let cleanup = receiver.close().await;
        outcome?;
        cleanup
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn rejected_or_stalled_exporters_preserve_commits_duplicates_and_settlement() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        scenarios::provision_broker(&url, &plan()?).await?;
        let owner = std::sync::Arc::new(Receiver::start().await?);
        let receiver = owner.clone();
        let outcome = database::with_database(|db| {
            Box::pin(async move {
                for (mode, id) in [(1, "telemetry-reject"), (2, "telemetry-stall")] {
                    receiver.mode(mode);
                    let before = receiver.records()?.len();
                    let bytes = bytes(id)?;
                    for (role, consumer, input, expected) in [
                        ("producer", false, bytes.as_slice(), "accepted"),
                        ("relay", false, &[][..], "published"),
                        ("consumer", true, &[][..], "applied"),
                        ("relay", true, &[][..], "published"),
                    ] {
                        assert_eq!(
                            observed(db, &url, &receiver.endpoint, role, consumer, b'c', input)
                                .await?
                                .0,
                            expected
                        );
                    }
                    republish(&url, &bytes).await?;
                    assert_eq!(
                        observed(db, &url, &receiver.endpoint, "consumer", true, b'c', &[])
                            .await?
                            .0,
                        "duplicate"
                    );
                    assert!(
                        receiver.records()?.len() > before,
                        "failure mode received actual export attempts"
                    );
                    assert_eq!(scenarios::state(db, id).await?.map(|s| s.0), Some(1));
                    let durable = scenarios::durable(&url).await?.info().await?.clone();
                    assert_eq!((durable.num_ack_pending, durable.num_pending), (0, 0));
                }
                for table in [
                    "edgeagent_message_inbox",
                    "conformance_probe",
                    "conformance_command",
                    "edgeagent_message_outbox",
                ] {
                    assert_eq!(db.count(true, table).await?, 2);
                }
                let unpublished: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {}.edgeagent_message_outbox WHERE published_at IS NULL",
                    db.consumer
                )))
                .fetch_one(&mut db.admin)
                .await?;
                assert_eq!(unpublished, 0);
                // A subsequent healthy process exports again; failure is not a
                // persistent domain mode or permission to create a new identity.
                receiver.mode(0);
                let before = receiver.records()?.len();
                republish(&url, &bytes("telemetry-stall")?).await?;
                assert_eq!(
                    observed(db, &url, &receiver.endpoint, "consumer", true, b'c', &[])
                        .await?
                        .0,
                    "duplicate"
                );
                assert!(
                    receiver.records()?.iter().skip(before).any(|r| r.accepted),
                    "healthy re-entry exports again"
                );
                Ok(())
            })
        })
        .await;
        let mut receiver =
            std::sync::Arc::try_unwrap(owner).map_err(|_| "receiver still borrowed")?;
        let cleanup = receiver.close().await;
        outcome?;
        cleanup
    })
    .await
}
