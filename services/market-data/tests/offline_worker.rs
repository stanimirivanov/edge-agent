//! Owned process composition proof; no vendor credentials or external endpoints.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned test schema inspection and cleanup."
)]
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata, evidence::*};
use edgeagent_market_data_application::{AcquisitionLedger, AcquisitionPublication};
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use futures_util::{FutureExt, TryStreamExt};
use object_store::ObjectStore;
use std::{path::Path, time::Duration};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn command(
    root: &Path,
    pg: &str,
    nats: &str,
    stream: &str,
    args: &[&str],
) -> Result<std::process::Output> {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-market-data"));
    eprintln!("offline proof phase: {}", args[0]);
    child
        .args(args)
        .env("USE_MOCK", "1")
        .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
        .env("EDGEAGENT_POSTGRES_URL", pg)
        .env("EDGEAGENT_NATS_URL", nats)
        .env("EDGEAGENT_OFFLINE_ROOT", root)
        .env("EDGEAGENT_OFFLINE_STREAM", stream)
        .env("EDGEAGENT_OFFLINE_CONSUMER", "offline")
        .env("EDGEAGENT_OFFLINE_MAX_MESSAGES", "1")
        .kill_on_drop(true);
    Ok(tokio::time::timeout(Duration::from_secs(120), child.output()).await??)
}
fn success(output: std::process::Output) -> Result<String> {
    if !output.status.success() {
        // This owned binary emits only bounded failure classes, never source errors.
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err("offline child failed (private output withheld)".into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
fn envelope(command: &AcquireEvidence, id: &str) -> Result<MessageEnvelope> {
    let d = DEFINITIONS[0];
    Ok(MessageEnvelope::from_payload(
        MessageMetadata {
            id: id.into(),
            source: Component::Gateway.source_uri().into(),
            message_type: d.message_type().into(),
            data_schema: d.data_schema().into(),
            subject: format!("acquisition/{}", command.request.as_str()),
            time: "2026-10-23T00:00:00Z".into(),
            correlation_id: "offline-test".into(),
            causation_id: "offline-parent".into(),
            idempotency_key: command.request.as_str().into(),
            partition_key: format!("acquisition/{}", command.request.as_str()),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        command,
    )?)
}

#[tokio::test]
#[ignore = "requires owned local PostgreSQL and NATS via EDGEAGENT_POSTGRES_URL and EDGEAGENT_NATS_URL"]
async fn offline_processes_commit_recover_relay_and_retire_without_provider_calls() -> Result {
    let pg = std::env::var("EDGEAGENT_POSTGRES_URL")?;
    let nats = std::env::var("EDGEAGENT_NATS_URL")?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("edgeagent-offline-{}-{unique}", std::process::id()));
    std::fs::create_dir(&root)?;
    let canonical = std::fs::canonicalize(&root)?;
    let schema = format!(
        "edgeagent_offline_{}",
        &DailyEvidenceArtifact::hash(canonical.to_str().ok_or("path")?.as_bytes()).as_str()[..32]
    );
    let stream_name = format!("offline_{}_{unique}", std::process::id());
    let client = async_nats::connect(&nats).await?;
    let context = async_nats::jetstream::new(client);
    let options: sqlx::postgres::PgConnectOptions = pg.parse()?;
    let pool =
        sqlx::PgPool::connect_with(options.options([("search_path", schema.as_str())])).await?;
    let test = std::panic::AssertUnwindSafe(async {
        success(command(&root, &pg, &nats, &stream_name, &["offline-init"]).await?)?;
        assert!(
            !command(&root, &pg, &nats, &stream_name, &["offline-init"])
                .await?
                .status
                .success()
        );
        let seeded = success(
            command(
                &root,
                &pg,
                &nats,
                &stream_name,
                &["offline-seed", "00000000000000000000000000000001"],
            )
            .await?,
        )?;
        let acquisition: AcquireEvidence = serde_json::from_str(&seeded)?;
        let command_subject = DEFINITIONS[0].subject()?;
        let event_subject = DEFINITIONS[1].subject()?;
        let stream = context
            .create_stream(async_nats::jetstream::stream::Config {
                name: stream_name.clone(),
                subjects: vec![command_subject.clone(), event_subject.clone()],
                max_messages: 16,
                max_bytes: 16_777_216,
                max_message_size: 1_048_576,
                storage: async_nats::jetstream::stream::StorageType::File,
                max_age: Duration::from_secs(600),
                ..Default::default()
            })
            .await?;
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("offline".into()),
                filter_subject: command_subject.clone(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                max_deliver: 3,
                max_ack_pending: 1,
                ..Default::default()
            })
            .await?;
        let delivery = envelope(&acquisition, "offline-first")?;
        context
            .publish(command_subject.clone(), delivery.to_json()?.into())
            .await?
            .await?;
        let first = success(command(&root, &pg, &nats, &stream_name, &["offline-worker"]).await?)?;
        assert!(first.contains("handled=1"));
        let ledger = PostgresAcquisitionLedger::new(pool.clone());
        let receipt = ledger.recover(&delivery).await?.ok_or("missing receipt")?;
        let store = object_store::local::LocalFileSystem::new_with_prefix(root.join("objects"))?;
        assert_eq!(store.list(None).try_collect::<Vec<_>>().await?.len(), 1);
        let retry = envelope(&acquisition, "offline-duplicate")?;
        context
            .publish(command_subject.clone(), retry.to_json()?.into())
            .await?
            .await?;
        let recovered =
            success(command(&root, &pg, &nats, &stream_name, &["offline-worker"]).await?)?;
        assert!(recovered.contains("retained_receipt=1"));
        assert_eq!(
            ledger
                .recover(&retry)
                .await?
                .ok_or("missing recovery")?
                .bytes(),
            receipt.bytes()
        );
        assert_eq!(store.list(None).try_collect::<Vec<_>>().await?.len(), 1);
        let (_, budget) = ledger.load(&acquisition).await?;
        assert_eq!(budget.attempts, 1);
        assert_eq!(budget.calls, 0);
        assert!(budget.pending.is_none());
        assert_eq!(budget.charged_bytes, 0);
        success(command(&root, &pg, &nats, &stream_name, &["offline-relay"]).await?)?;
        let published = stream
            .get_last_raw_message_by_subject(&event_subject)
            .await?;
        assert_eq!(published.payload.as_ref(), receipt.bytes());
        let mut conflicting = acquisition.clone();
        conflicting.request_sha256 = DailyEvidenceArtifact::hash(b"conflicting offline admission");
        context
            .publish(
                command_subject.clone(),
                envelope(&conflicting, "offline-poison")?.to_json()?.into(),
            )
            .await?
            .await?;
        let poison = command(&root, &pg, &nats, &stream_name, &["offline-worker"]).await?;
        assert!(!poison.status.success());
        let quarantined: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_quarantine")
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            quarantined, 1,
            "confirmed quarantine precedes broker termination"
        );
        ledger
            .revoke_evidence_scope(&ledger.load(&acquisition).await?.0.retention_scope()?)
            .await?;
        context
            .publish(
                command_subject,
                envelope(&acquisition, "offline-revoked")?.to_json()?.into(),
            )
            .await?
            .await?;
        let denied = success(command(&root, &pg, &nats, &stream_name, &["offline-worker"]).await?)?;
        assert!(denied.contains("retained_denial=1"));
        assert_eq!(
            store.list(None).try_collect::<Vec<_>>().await?.len(),
            1,
            "restart cannot infer write quiescence"
        );
        success(
            command(
                &root,
                &pg,
                &nats,
                &stream_name,
                &[
                    "offline-maintain",
                    "--owner-confirmed-quiescence",
                    acquisition.request.as_str(),
                ],
            )
            .await?,
        )?;
        assert!(store.list(None).try_collect::<Vec<_>>().await?.is_empty());
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .catch_unwind()
    .await;
    // Only these freshly created, validated test targets may be destroyed.
    let stream_cleanup = context.delete_stream(&stream_name).await;
    let schema_cleanup = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&pool)
    .await;
    pool.close().await;
    let files_cleanup = std::fs::remove_dir_all(&root);
    match test {
        Ok(result) => result?,
        Err(panic) => std::panic::resume_unwind(panic),
    }
    stream_cleanup?;
    schema_cleanup?;
    files_cleanup?;
    Ok(())
}
