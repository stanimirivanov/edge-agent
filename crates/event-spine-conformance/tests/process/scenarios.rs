use super::{broker, database, plan};
use broker::{TestError, TestResult};
use edgeagent_contracts::{Component, MessageMetadata};
use edgeagent_event_spine_conformance::{ApplyProbe, definitions};
use edgeagent_messaging_nats::topology::{TopologyPlan, provision};
use sqlx::PgConnection;
use std::time::Duration;

pub async fn provision_broker(url: &str, topology: &TopologyPlan) -> TestResult {
    provision(
        &broker::context(url, topology.provisioner_permissions()).await?,
        topology,
    )
    .await?;
    Ok(())
}

pub async fn command(
    db: &database::Database,
    url: &str,
    id: &str,
    probe: &str,
    revision: u32,
    value: u32,
) -> TestResult {
    let definition = definitions()[0];
    let bytes = ApplyProbe::new(probe, revision, value)?
        .envelope(MessageMetadata {
            id: id.into(),
            source: Component::Gateway.source_uri().into(),
            message_type: definition.message_type().into(),
            subject: format!("probe/{probe}"),
            time: "2026-10-17T00:00:00Z".into(),
            data_schema: definition.data_schema().into(),
            correlation_id: "m02-ordering".into(),
            causation_id: "request-ordering".into(),
            idempotency_key: id.into(),
            partition_key: format!("probe/{probe}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        })?
        .to_json()?;
    let mut child = db.spawn("producer", false, url).await?;
    child.begin(b'c', &bytes).await?;
    assert_eq!(child.finish().await?, "accepted");
    assert_eq!(db.worker("relay", false, url, true).await?, "published");
    Ok(())
}

pub async fn held(db: &database::Database, url: &str) -> Result<database::Worker, TestError> {
    let mut worker = db.spawn("consumer", true, url).await?;
    worker.begin(b'h', &[]).await?;
    worker.expect("delivered").await?;
    Ok(worker)
}

pub async fn release(mut worker: database::Worker) -> Result<String, TestError> {
    worker.begin(b'c', &[]).await?;
    worker.finish().await
}

pub async fn state(
    db: &mut database::Database,
    probe: &str,
) -> Result<Option<(i64, i64)>, TestError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT revision, value FROM {}.conformance_probe WHERE probe_id = $1",
        db.consumer
    )))
    .bind(probe)
    .fetch_optional(&mut db.admin)
    .await?)
}

pub async fn quarantine(
    db: &mut database::Database,
) -> Result<Vec<(String, i64, Vec<u8>)>, TestError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT failure_code, last_delivery_attempt, payload FROM {}.edgeagent_message_quarantine ORDER BY failure_code", db.consumer
    ))).fetch_all(&mut db.admin).await?)
}

pub async fn wait_for_lock(admin: &mut PgConnection, application: &str) -> TestResult {
    // Poll only a real lock-wait observation. The deadline is a liveness guard,
    // not a latency benchmark or a sleep used to guess transaction progress.
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut poll = tokio::time::interval(Duration::from_millis(10));
        loop {
            poll.tick().await;
            let locked: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock')"
            ).bind(application).fetch_one(&mut *admin).await?;
            if locked { return Ok::<(), TestError>(()); }
        }
    }).await??;
    Ok(())
}

pub async fn durable(
    url: &str,
) -> Result<async_nats::jetstream::consumer::PullConsumer, TestError> {
    let topology = plan()?;
    let stream = topology
        .streams()
        .iter()
        .find(|s| s.definition() == definitions()[0])
        .ok_or("command stream missing")?;
    let name = stream
        .consumers()
        .first()
        .and_then(|c| c.config().durable_name.as_deref())
        .ok_or("durable missing")?;
    let context =
        async_nats::jetstream::new(broker::connect(url, "inspector", "_INBOX.inspector").await?);
    context
        .get_stream(&stream.config().name)
        .await?
        .get_consumer(name)
        .await
}
