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
    use tokio::io::AsyncWriteExt as _;
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
        sqlx::query("UPDATE edgeagent_message_inbox SET message_type='synthetic-corrupt-type'").execute(&db.pool).await?;
        let damaged=snapshot(&db).await?;
        assert!(!replay(&profile,&db.schema,&input,None).await?.status.success());
        assert_eq!(snapshot(&db).await?,damaged);
        sqlx::query("UPDATE edgeagent_message_inbox SET message_type=$1").bind(event.message_type()).execute(&db.pool).await?;
        let before=snapshot(&db).await?;
        for _ in 0..2 {
            let output=replay(&profile,&db.schema,&input,None).await?;
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout,original.stdout);
        }
        for key in ["EDGEAGENT_RESEARCH_REPLAY_RENDERER","EDGEAGENT_RESEARCH_REPLAY_MOMENTUM","EDGEAGENT_RESEARCH_IMPLEMENTATION","EDGEAGENT_RESEARCH_CONFIGURATION"] {
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
async fn declared_abstention_replay_checks_profile_without_reading_evidence() -> Result {
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
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
        }
        assert_eq!(snapshot(&db).await?, before);
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
