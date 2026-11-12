//! Real scheduler/worker, owner authority, projection, Gateway queries and replay.
use super::*;
mod model;
mod recovery;
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact,
    explanation::{Companion, Fallback, Outcome},
    explanation_profile::Profile,
};
const ACQUISITION: &str = "00000000000000000000000000000001";
const AUDIENCE: &str = "00000000000000000000000000000002";
fn profile() -> Result<Profile> {
    Ok(serde_json::from_value(serde_json::json!({
        "deployment":{"runtime":"a".repeat(64),"weights":"b".repeat(64)},
        "settings":{"seed":1,"temperature_milli":0,"max_output_tokens":512,"timeout_ms":1000},
        "prompt":DailyEvidenceArtifact::hash(b"Select only the supplied synthetic claims.").as_str(), "tools":"none",
        "validator":DailyEvidenceArtifact::hash(b"research-explanation-allowlist-v1").as_str(),
        "renderer":DailyEvidenceArtifact::hash(b"research-explanation-trace.v1").as_str()
    }))?)
}
async fn output(mut command: tokio::process::Command) -> Result<Vec<u8>> {
    let output = tokio::time::timeout(Duration::from_secs(40), command.output()).await??;
    if !output.status.success() {
        return Err(format!(
            "explanation command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and four freshly built service binaries via research-processes gate"]
async fn scheduled_fallback_queries_and_replay_preserve_original_artifact() -> Result {
    journey(None).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and real service binaries via research-processes gate"]
async fn controlled_generation_queries_and_revocation_preserve_original_artifact() -> Result {
    journey(Some("valid")).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and real service binaries via research-processes gate"]
async fn controlled_injection_is_rejected_and_falls_back_through_queries() -> Result {
    journey(Some("injection")).await
}
async fn journey(controlled: Option<&str>) -> Result {
    let mut h = Journey::new().await?;
    let tested = AssertUnwindSafe(async {
        h.seed_history().await?;
        let address = address()?;
        let origin = format!("http://{address}");
        h.start_query_servers(None, Some((&origin, AUDIENCE)))
            .await?;
        submit(&h, 5).await?;
        h.run("research", "offline-worker", &[]).await?;
        let original = result(&h, "artifact").await?;
        // The real PUBLISHED broker delivery must admit the durable job.
        let mut scheduler = h.command("research", "offline-explanation-scheduler")?;
        scheduler.env("EDGEAGENT_RESEARCH_CONSUMER", "explanation-jobs");
        output(scheduler).await?;
        let store =
            edgeagent_research_postgres::PostgresResearchStore::new(h.research.pool.clone());
        let job = store
            .explanation_job("operator", None)
            .await?
            .ok_or("scheduled job")?;
        let id = job.job.id()?;
        let ResearchRecord::Artifact(artifact) = ResearchRecord::decode(&original)? else {
            return Err("artifact".into());
        };
        let grant = serde_json::json!({
            "kind":"research.explanation.grant.v1", "anchor":job.job.anchor,
            "audience":AUDIENCE,"purpose":"explanation_serving","document":null,
            "revision":1,"valid_from_ms":artifact.request.decision_at_ms,
            "expires_at_ms":artifact.expires_at_ms,"revoked":false
        });
        h.input(
            "research",
            "offline-explanation-grant",
            &serde_json::to_vec(&grant)?,
        )
        .await?;
        if controlled.is_some() {
            let mut model_grant = grant.clone();
            model_grant["purpose"] = "model_disclosure".into();
            h.input(
                "research",
                "offline-explanation-grant",
                &serde_json::to_vec(&model_grant)?,
            )
            .await?;
        }
        let review = |revision, decision: &str| {
            serde_json::json!({
            "audience":AUDIENCE,"agreement":ACQUISITION,"revision":revision,"decision":decision,
            "valid_from_ms":artifact.request.decision_at_ms,"expires_at_ms":artifact.expires_at_ms
        }).to_string()
        };
        h.input_with_args(
            "market",
            "offline-explanation-review",
            &[ACQUISITION],
            review(1, "allow").as_bytes(),
        )
        .await?;
        let path = h.root.join("explanation.json");
        let config = profile()?;
        std::fs::write(&path, config.canonical_bytes()?)?;
        let decision = edgeagent_research_application::publication::decode_decision(
            job.original.receipt.decision().ok_or("decision")?,
        )?;
        let prepared = edgeagent_research_application::explanation_worker::Prepared::new(
            job.original.receipt.record(),
            &decision,
            &config,
        )?;
        let model = model::Server::start(
            &h.root,
            prepared.input(),
            controlled,
            &config.deployment.weights,
        )
        .await?;
        let configure = |mut command: tokio::process::Command| {
            command
                .env("EDGEAGENT_RESEARCH_EXPLANATION_PROFILE", &path)
                .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE)
                .env("EDGEAGENT_RESEARCH_EXPLANATION_JOB", &id)
                .env(
                    "EDGEAGENT_RESEARCH_REPLAY_RENDERER",
                    edgeagent_research_application::RENDERER_REVISION,
                )
                .env(
                    "EDGEAGENT_RESEARCH_REPLAY_MOMENTUM",
                    edgeagent_research_domain::momentum::REVISION,
                )
                .env_remove("EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION")
                .env_remove("EDGEAGENT_RESEARCH_OWNER_ACQUISITION");
            command
        };
        std::fs::write(
            h.root.join("synthetic-explanation-proof"),
            b"PR19 invented fixtures only\n",
        )?;
        let companion = if let Some(mode) = controlled {
            let mut refused = configure(h.command("qualification", mode)?);
            refused
                .env("EDGEAGENT_RESEARCH_OWNER_ACQUISITION", ACQUISITION)
                .env_remove("EDGEAGENT_SYNTHETIC_EXPLANATION_PROOF");
            let refused = refused.output().await?;
            assert!(!refused.status.success() && refused.stdout.is_empty());
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_attempts")
                    .fetch_one(&h.research.pool)
                    .await?,
                0
            );
            let mut command = configure(h.command("qualification", mode)?);
            command
                .env("EDGEAGENT_SYNTHETIC_EXPLANATION_PROOF", "1")
                .env("EDGEAGENT_RESEARCH_OWNER_ACQUISITION", ACQUISITION);
            output(command).await?
        } else {
            output(configure(
                h.command("research", "offline-explanation-worker")?,
            ))
            .await?
        };
        let decoded = Companion::decode(&companion)?;
        match (controlled, &decoded.outcome) {
            (
                None,
                Outcome::Fallback {
                    reason: Fallback::Disabled,
                    ..
                },
            ) => {}
            (Some("valid"), Outcome::Generated { proposal }) => {
                assert!(proposal.claims.iter().any(|claim| claim.variant
                    == edgeagent_contracts::explanation::Variant::PlainLanguage))
            }
            (
                Some("injection"),
                Outcome::Fallback {
                    reason: Fallback::InvalidProposal,
                    ..
                },
            ) => {}
            _ => return Err("unexpected qualified outcome".into()),
        }
        assert!(decoded.binding.anchor == job.job.anchor);
        assert_eq!(decoded.binding.profile, config.id()?);
        assert_eq!(decoded.binding.evidence, prepared.binding().evidence);
        assert_eq!(
            output(configure(
                h.command("research", "offline-explanation-worker")?
            ))
            .await?,
            companion
        );
        let mut server = h.command("research", "offline-explanation-authority")?;
        server
            .env("EDGEAGENT_RESEARCH_EXPLANATION_LISTEN", address.to_string())
            .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE);
        let mut server = server.spawn()?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if tokio::net::TcpStream::connect(address).await.is_ok() {
                    break;
                }
                if server.try_wait()?.is_some() {
                    return Err("authority exited".into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Result::Ok(())
        })
        .await??;
        let query = "/v1/research-requests/operator/qualification/explanation";
        assert_eq!(h.request("GET", query, &[]).await?.0, 404);
        h.run("research", "offline-relay", &[]).await?;
        h.run("projector", "offline-worker", &[]).await?;
        assert_eq!(
            h.request("GET", query, &[]).await?,
            (200, companion.clone())
        );
        assert_eq!(
            h.request(
                "GET",
                "/v1/research-requests/operator/qualification/outcome",
                &[]
            )
            .await?,
            (200, original.clone())
        );
        let calls = usize::from(controlled.is_some());
        assert_eq!(model.calls(), calls);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_attempts")
                .fetch_one(&h.research.pool)
                .await?,
            if controlled.is_some() { 1 } else { 0 }
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_completions")
                .fetch_one(&h.research.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_outbox")
                .fetch_one(&h.research.pool)
                .await?,
            3
        );
        let before = retained(&h).await?;
        assert!(
            before
                .iter()
                .all(|row| !row.contains("private-injection-sentinel"))
        );
        assert_eq!(
            output(configure(
                h.command("research", "offline-explanation-replay")?
            ))
            .await?,
            companion
        );
        assert_eq!(retained(&h).await?, before);
        h.input_with_args(
            "market",
            "offline-explanation-review",
            &[ACQUISITION],
            review(2, "revoke").as_bytes(),
        )
        .await?;
        assert_eq!(
            h.request("GET", query, &[]).await?,
            (403, br#"{"status":"forbidden"}"#.to_vec())
        );
        assert_eq!(
            h.request(
                "GET",
                "/v1/research-requests/operator/qualification/outcome",
                &[]
            )
            .await?,
            (200, original)
        );
        assert_eq!(retained(&h).await?, before);
        assert_eq!(model.calls(), calls);
        server.start_kill()?;
        server.wait().await?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(tested).await
}
async fn retained(h: &Journey) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    for table in [
        "edgeagent_research_completions",
        "edgeagent_research_decisions",
        "edgeagent_explanation_attempts",
        "edgeagent_explanation_completions",
        "edgeagent_research_explanation_jobs",
        "edgeagent_message_outbox",
    ] {
        rows.push(sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]'::jsonb)::text FROM {table} t"))).fetch_one(&h.research.pool).await?);
    }
    Ok(rows)
}
