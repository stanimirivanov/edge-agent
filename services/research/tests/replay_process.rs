//! Real read-only receipt and independent replay processes; no second publication.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated owned schema inspection/fault injection."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::governed_event as event;
use support::*;
async fn snapshot(db: &Database) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    for table in [
        "edgeagent_research_completions",
        "edgeagent_research_decisions",
        "edgeagent_research_approvals",
        "edgeagent_research_controls",
        "edgeagent_research_lifecycle",
        "edgeagent_message_inbox",
        "edgeagent_message_outbox",
    ] {
        rows.push(sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]'::jsonb)::text FROM {table} t"))).fetch_one(&db.pool).await?);
    }
    Ok(rows)
}
async fn replay(
    profile: &Profile,
    schema: &str,
    input: &[u8],
    change: Option<(&str, &str)>,
) -> Result<std::process::Output> {
    let mut command = profile.command(schema, "offline-replay");
    command
        .env(
            "EDGEAGENT_RESEARCH_REPLAY_RENDERER",
            edgeagent_research_application::RENDERER_REVISION,
        )
        .env(
            "EDGEAGENT_RESEARCH_REPLAY_MOMENTUM",
            edgeagent_research_domain::momentum::REVISION,
        );
    if let Some((key, value)) = change {
        command.env(key, value);
    }
    invoke(command, input).await
}

async fn explain(profile: &Profile, schema: &str, input: &[u8]) -> Result<std::process::Output> {
    let mut command = profile.command(schema, "offline-explain");
    command
        .env(
            "EDGEAGENT_RESEARCH_REPLAY_RENDERER",
            edgeagent_research_application::RENDERER_REVISION,
        )
        .env(
            "EDGEAGENT_RESEARCH_REPLAY_MOMENTUM",
            edgeagent_research_domain::momentum::REVISION,
        );
    invoke(command, input).await
}

async fn invoke(command: std::process::Command, input: &[u8]) -> Result<std::process::Output> {
    use tokio::io::AsyncWriteExt as _;
    let mut child = tokio::process::Command::from(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    child.stdin.take().ok_or("stdin")?.write_all(input).await?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await??,
    )
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn replay_restart_and_refusals_preserve_all_retained_rows() -> Result {
    let db = Database::new().await?;
    let result=AssertUnwindSafe(async {
        let (event,bytes)=event("replay")?;let profile=Profile::new(&bytes)?;
        let input=event.to_json()?;
        let original=run(&profile,&db.schema,&input).await?;assert!(original.status.success());
        let before_trace = snapshot(&db).await?;
        let trace = explain(&profile, &db.schema, &input).await?;
        assert!(trace.status.success(), "{}", String::from_utf8_lossy(&trace.stderr));
        let text = std::str::from_utf8(&trace.stdout)?;
        assert!(text.contains("score_ppm=") && text.contains("evidence="));
        assert!(text.ends_with("Research only. This explanation confers no trading authority.\n"));
        assert_eq!(explain(&profile, &db.schema, &input).await?.stdout, trace.stdout);
        assert_eq!(snapshot(&db).await?, before_trace);
        let (foreign, _) = support::governed_event("foreign-trace")?;
        let refused = explain(&profile, &db.schema, &foreign.to_json()?).await?;
        assert!(!refused.status.success() && refused.stdout.is_empty());

        sqlx::query("UPDATE edgeagent_message_inbox SET message_type='synthetic-corrupt-type'").execute(&db.pool).await?;
        let damaged=snapshot(&db).await?;
        assert!(!replay(&profile,&db.schema,&input,None).await?.status.success());
        assert_eq!(snapshot(&db).await?,damaged);
        sqlx::query("UPDATE edgeagent_message_inbox SET message_type=$1").bind(event.message_type()).execute(&db.pool).await?;
        let store=edgeagent_research_postgres::PostgresResearchStore::new(db.pool.clone());
        store.govern(&governance_profile()?,3).await?;
        store.control(&edgeagent_contracts::research_controls::Command::V1 {
            scope:"operator".into(), target:edgeagent_contracts::research_controls::Target::Scope,
            revision:1, enabled:false, reason_code:"replay-proof".into(),
        }).await?;
        let before=snapshot(&db).await?;
        for _ in 0..2 {
            let output=replay(&profile,&db.schema,&input,None).await?;
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout,original.stdout);
        }
        for key in ["EDGEAGENT_RESEARCH_REPLAY_RENDERER","EDGEAGENT_RESEARCH_REPLAY_MOMENTUM"] {
            let output=replay(&profile,&db.schema,&input,Some((key,&"09".repeat(32)))).await?;
            assert!(!output.status.success());assert!(output.stdout.is_empty());
        }
        let mut changed:serde_json::Value=serde_json::from_slice(&input)?;changed["id"]="new-delivery".into();
        assert!(!replay(&profile,&db.schema,&serde_json::to_vec(&changed)?,None).await?.status.success());
        let (unknown,_)=support::governed_event("unknown")?;
        assert!(!replay(&profile,&db.schema,&unknown.to_json()?,None).await?.status.success());
        for exit in [75,77] {
            std::fs::write(&profile.owner,format!("#!/bin/sh\nexit {exit}\n"))?;
            let output=replay(&profile,&db.schema,&input,None).await?;
            assert!(!output.status.success());assert!(output.stdout.is_empty());
            let refused = explain(&profile, &db.schema, &input).await?;
            assert!(!refused.status.success() && refused.stdout.is_empty());
        }
        std::fs::write(&profile.owner,format!("#!/bin/sh\n/bin/cat '{}'\n",profile.artifact.display()))?;
        std::fs::write(&profile.artifact,b"corrupt invented evidence")?;
        let output=replay(&profile,&db.schema,&input,None).await?;
        assert!(!output.status.success());assert!(output.stdout.is_empty());
        assert_eq!(snapshot(&db).await?,before);
        // Fault only this owned fixture, then verify corrupt retained output is never relabeled.
        sqlx::raw_sql("DROP TRIGGER edgeagent_research_immutable ON edgeagent_research_completions; UPDATE edgeagent_research_completions SET outcome=decode('01','hex')").execute(&db.pool).await?;
        let corrupt=snapshot(&db).await?;
        assert!(!replay(&profile,&db.schema,&input,None).await?.status.success());
        assert_eq!(snapshot(&db).await?,corrupt);
        Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn declared_abstention_replay_uses_retained_profile_without_evidence_reads() -> Result {
    use edgeagent_contracts::{
        MessageEnvelope, research::AbstentionReason, research_acquisition::ResearchEvidence,
    };
    let db = Database::new().await?;
    let result = AssertUnwindSafe(async {
        let (event, bytes) = event("declined")?;
        let profile = Profile::new(&bytes)?;
        profile.unavailable()?;
        let request = edgeagent_research_application::publication::evaluation_request(&event)?;
        let mut value: serde_json::Value = serde_json::from_slice(&event.to_json()?)?;
        value["data"] = serde_json::to_value(ResearchEvidence::Abstained {
            request,
            reason: AbstentionReason::InsufficientHistory,
        })?;
        let input = MessageEnvelope::from_json(&serde_json::to_vec(&value)?)?.to_json()?;
        let original = run(&profile, &db.schema, &input).await?;
        assert!(original.status.success());
        let before = snapshot(&db).await?;
        let result = replay(&profile, &db.schema, &input, None).await?;
        assert!(result.status.success());
        assert_eq!(result.stdout, original.stdout);
        for key in [
            "EDGEAGENT_RESEARCH_IMPLEMENTATION",
            "EDGEAGENT_RESEARCH_CONFIGURATION",
        ] {
            let result =
                replay(&profile, &db.schema, &input, Some((key, &"09".repeat(32)))).await?;
            assert!(result.status.success());
            assert_eq!(result.stdout, original.stdout);
        }
        assert_eq!(snapshot(&db).await?, before);
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn pinned_policy_abstention_replays_without_active_configuration() -> Result {
    use edgeagent_contracts::{
        research::{AbstentionReason, ResearchRecord},
        strategy::{Configuration, ExecutionConfiguration},
    };
    use edgeagent_research_application::execution::Selection;
    let db = Database::new().await?;
    let result = AssertUnwindSafe(async {
        let mut wire = execution()?;
        let ExecutionConfiguration::V1 { bundle, .. } = &mut wire;
        let Configuration::V1 { policy, .. } = bundle;
        policy.min_daily_turnover_cents = u64::MAX;
        let configuration = wire.canonical_bytes()?;
        let binding = Selection::decode(&configuration)?.profile("operator");
        let (event, bytes) = event_with_configuration("policy-replay", binding.configuration.clone())?;
        let profile = Profile::new(&bytes)?;
        let path = profile.root.join("execution.json");
        std::fs::write(&path, configuration)?;
        let store = edgeagent_research_postgres::PostgresResearchStore::new(db.pool.clone());
        store.govern(&binding, 1).await?;
        store.promote(&promotion_fixture::retain(&store, &binding).await?).await?;
        let mut worker = profile.command(&db.schema, "offline-once");
        worker.env("EDGEAGENT_RESEARCH_CONFIGURATION", &binding.configuration);
        let input = event.to_json()?;
        let original = invoke(worker, &input).await?;
        assert!(original.status.success(), "{}", String::from_utf8_lossy(&original.stderr));
        assert!(matches!(ResearchRecord::decode(&original.stdout)?, ResearchRecord::Abstention(a) if a.reason == AbstentionReason::NoEligibleCandidate));
        let before = snapshot(&db).await?;
        std::fs::remove_file(&path)?;
        let output = replay(&profile, &db.schema, &input, Some(("EDGEAGENT_RESEARCH_TTL_MS", "invalid-current-ttl"))).await?;
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, original.stdout);
        // The added event changes policy reasons while liquidity still denies the only candidate.
        let owner_policy = profile.root.join("policy.py");
        let changed = std::fs::read_to_string(&owner_policy)?
            .replace(r#""events": []"#, r#""events": [request["decision_at_ms"]]"#);
        std::fs::write(owner_policy, changed)?;
        let drift = replay(&profile, &db.schema, &input, None).await?;
        assert!(!drift.status.success());
        assert!(drift.stdout.is_empty());
        assert_eq!(snapshot(&db).await?, before);
        Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}
