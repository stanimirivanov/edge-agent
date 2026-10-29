//! Actual JetStream settlement and correlated outbox publication through separate workers.
#![allow(clippy::disallowed_methods, reason = "Owned test schema inspection.")]
mod research_support;
use edgeagent_contracts::{
    MessageEnvelope,
    research_acquisition::{self, ResearchEvidence},
};
use futures_util::{FutureExt, StreamExt};
use research_support::*;
use std::time::Duration;
async fn worker(
    h: &Harness,
    nats: &str,
    stream: &str,
    relay: bool,
) -> Result<std::process::Output> {
    let args = if relay {
        vec!["offline-relay"]
    } else {
        vec![
            "offline-research-worker",
            "00000000000000000000000000000001",
        ]
    };
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-market-data"))
        .args(args)
        .env("USE_MOCK", "1")
        .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
        .env("EDGEAGENT_POSTGRES_URL", &h.pg)
        .env("EDGEAGENT_OFFLINE_ROOT", &h.root)
        .env("EDGEAGENT_OFFLINE_RESEARCH_SCOPE", "operator")
        .env("EDGEAGENT_NATS_URL", nats)
        .env("EDGEAGENT_OFFLINE_STREAM", stream)
        .env("EDGEAGENT_OFFLINE_CONSUMER", "research")
        .env(
            "EDGEAGENT_OFFLINE_MAX_MESSAGES",
            if relay { "16" } else { "1" },
        )
        .kill_on_drop(true)
        .output();
    Ok(tokio::time::timeout(Duration::from_secs(60), child).await??)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL and NATS via EDGEAGENT_POSTGRES_URL and EDGEAGENT_NATS_URL"]
async fn broker_worker_commits_before_ack_relays_exact_outcome_and_quarantines_bad_correlation()
-> Result {
    let h = Harness::new().await?;
    let nats = std::env::var("EDGEAGENT_NATS_URL")?;
    let url = url::Url::parse(&nats)?;
    if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) || url.query().is_some()
    {
        return Err("test broker must be loopback".into());
    }
    let name = h.schema.clone();
    let client = async_nats::connect(&nats).await?;
    let context = async_nats::jetstream::new(client.clone());
    let result = std::panic::AssertUnwindSafe(async {
        let request = h.seed("history-v1").await?;
        let first = delivery(&request, "first")?;
        let subject = research_acquisition::REQUEST.subject()?;
        let stream = context
            .create_stream(async_nats::jetstream::stream::Config {
                name: name.clone(),
                subjects: vec![
                    subject.clone(),
                    research_acquisition::DECIDED.subject()?,
                    edgeagent_contracts::evidence::DEFINITIONS[1].subject()?,
                ],
                max_messages: 32,
                max_bytes: 16_777_216,
                max_message_size: 262_144,
                max_age: Duration::from_secs(600),
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await?;
        let mut consumer = stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("research".into()),
                filter_subject: subject.clone(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                max_deliver: 3,
                max_ack_pending: 1,
                ..Default::default()
            })
            .await?;
        for envelope in [first.clone(), delivery(&request, "duplicate")?] {
            context
                .publish(subject.clone(), envelope.to_json()?.into())
                .await?
                .await?;
            let result = worker(&h, &nats, &name, false).await?;
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(consumer.info().await?.num_ack_pending, 0);
        }
        let original = h.once(&first).await?;
        assert!(matches!(
            ResearchEvidence::from_envelope(&original)?,
            ResearchEvidence::Ready { .. }
        ));
        let mut events = client
            .subscribe(research_acquisition::DECIDED.subject()?)
            .await?;
        client.flush().await?;
        let relay = worker(&h, &nats, &name, true).await?;
        assert!(
            relay.status.success(),
            "{}",
            String::from_utf8_lossy(&relay.stderr)
        );
        let message = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await?
            .ok_or("missing relayed outcome")?;
        assert_eq!(
            MessageEnvelope::from_json(&message.payload)?.to_json()?,
            original.to_json()?
        );
        let mut poison: serde_json::Value = serde_json::from_slice(&first.to_json()?)?;
        poison["correlationid"] = "foreign".into();
        context
            .publish(subject, serde_json::to_vec(&poison)?.into())
            .await?
            .await?;
        assert!(!worker(&h, &nats, &name, false).await?.status.success());
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_quarantine")
                .fetch_one(&h.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_evidence")
                .fetch_one(&h.pool)
                .await?,
            1
        );
        let bridge = edgeagent_market_data_postgres::PostgresResearchEvidence::new(h.pool.clone());
        let owner = bridge
            .owner_record(&edgeagent_contracts::evidence::EvidenceId::new(
                "00000000000000000000000000000001",
            )?)
            .await?;
        let ledger = edgeagent_market_data_postgres::PostgresAcquisitionLedger::new(h.pool.clone());
        ledger
            .revoke_evidence_scope(&owner.retention_scope()?)
            .await?;
        ledger.retire_due_evidence(16).await?;
        assert!(bridge.denied(&first).await?);
        context
            .publish(
                research_acquisition::REQUEST.subject()?,
                first.to_json()?.into(),
            )
            .await?
            .await?;
        assert!(worker(&h, &nats, &name, false).await?.status.success());
        assert_eq!(consumer.info().await?.num_ack_pending, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_evidence")
                .fetch_one(&h.pool)
                .await?,
            0
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    let _ = context.delete_stream(&name).await;
    client.drain().await?;
    h.finish(result).await
}
