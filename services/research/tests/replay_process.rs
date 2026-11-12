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
        "edgeagent_explanation_grants",
        "edgeagent_explanation_attempts",
        "edgeagent_explanation_completions",
        "edgeagent_research_explanation_jobs",
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

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn explanation_replay_revalidates_without_model_publication_or_mutation() -> Result {
    use edgeagent_contracts::{
        explanation::{Companion, Variant},
        explanation_profile::Profile as ExplanationProfile,
    };
    use edgeagent_research_application::{
        explanation::Allowlist,
        explanation_context::{Grant, Purpose, Request},
        explanation_worker::Prepared,
        publication::decode_decision,
    };
    use edgeagent_research_postgres::{
        PostgresResearchStore,
        explanation_attempts::{Intent, ReservationOutcome},
        explanation_completion::Completion,
    };
    let db = Database::new().await?;
    let result = AssertUnwindSafe(async {
        let (event, bytes) = event("explanation-replay")?;
        let owner = Profile::new(&bytes)?;
        let input = event.to_json()?;
        let original = run(&owner, &db.schema, &input).await?;
        assert!(original.status.success());
        let store = PostgresResearchStore::new(db.pool.clone());
        let retained = store.retained_for_replay(&event).await?.ok_or("original")?;
        let decision = decode_decision(retained.receipt.decision().ok_or("decision")?)?;
        let hash = edgeagent_contracts::evidence::DailyEvidenceArtifact::hash;
        let profile: ExplanationProfile = serde_json::from_value(serde_json::json!({
            "deployment":{"runtime":"a".repeat(64),"weights":"b".repeat(64)},
            "settings":{"seed":1,"temperature_milli":0,"max_output_tokens":512,"timeout_ms":1000},
            "prompt":"c".repeat(64), "tools":"none",
            "validator":hash(b"research-explanation-allowlist-v1").as_str(),
            "renderer":hash(b"research-explanation-trace.v1").as_str()
        }))?;
        let prepared = Prepared::new(retained.receipt.record(), &decision, &profile)?;
        let allowlist = Allowlist::from_verified(retained.receipt.record(), &decision)?;
        let mut proposal = allowlist.standard()?.proposal().clone();
        proposal.claims[0].variant = Variant::PlainLanguage;
        let validated = allowlist.validate(&proposal.canonical_bytes()?)?;
        let now = chrono::Utc::now().timestamp_millis();
        let mut grant = Grant {request: Request {anchor: prepared.binding().anchor.clone(),
            audience:"00000000000000000000000000000002".into(), purpose: Purpose::ModelDisclosure, document: None},
            valid_from_ms: now-1000, expires_at_ms: now+60000, revoked:false};
        store.set_explanation_grant(&grant,1).await?;
        let intent = Intent {binding: prepared.binding().clone(),input_digest:prepared.input_digest(),audience:grant.request.audience.clone(),deadline_ms:now+30000};
        let ReservationOutcome::Reserved(permit) = store.reserve_explanation_attempt(&intent, || now).await? else {return Err("permit".into())};
        grant.request.purpose = Purpose::ExplanationServing;
        store.set_explanation_grant(&grant,1).await?;
        let receipt = store.complete_explanation(&publication_request(&event)?, prepared.binding(),
            Completion::Generated {explanation:&validated,permit:&permit},Some(&grant), || now,"2026-11-11T00:00:00Z").await?;
        store.admit_explanation_job("operator",retained.receipt.publication()).await?;
        let job = Companion::decode(receipt.bytes())?.job().id()?;
        let profile_path = owner.root.join("explanation.json");
        std::fs::write(&profile_path,profile.canonical_bytes()?)?;
        // No model server, runtime executable, weights or prompt is installed.
        // The replay application accepts no model/attempt/publication capability.
        let command = || {
            let mut command = owner.command(&db.schema,"offline-explanation-replay");
            command.env("EDGEAGENT_RESEARCH_EXPLANATION_JOB",&job)
                .env("EDGEAGENT_RESEARCH_EXPLANATION_PROFILE",&profile_path)
                .env("EDGEAGENT_RESEARCH_REPLAY_RENDERER",edgeagent_research_application::RENDERER_REVISION)
                .env("EDGEAGENT_RESEARCH_REPLAY_MOMENTUM",edgeagent_research_domain::momentum::REVISION)
                .env_remove("EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION")
                .env_remove("EDGEAGENT_RESEARCH_OWNER_ACQUISITION");
            command
        };
        let before = snapshot(&db).await?;
        for _ in 0..2 {
            let output = invoke(command(),b"").await?;
            assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout,receipt.bytes());
        }
        for key in ["EDGEAGENT_RESEARCH_REPLAY_RENDERER","EDGEAGENT_RESEARCH_REPLAY_MOMENTUM","EDGEAGENT_RESEARCH_EXPLANATION_JOB","EDGEAGENT_RESEARCH_SCOPE"] {
            let mut changed = command(); changed.env(key,"e".repeat(64));
            let output = invoke(changed,b"").await?;
            assert!(!output.status.success() && output.stdout.is_empty());
        }
        let mut changed = profile.clone(); changed.prompt = "e".repeat(64);
        std::fs::write(&profile_path,changed.canonical_bytes()?)?;
        let output = invoke(command(),b"").await?;
        assert!(!output.status.success() && output.stdout.is_empty());
        std::fs::write(&profile_path,profile.canonical_bytes()?)?;
        let owner_script = std::fs::read(&owner.owner)?;
        for exit in [75,77] {
            std::fs::write(&owner.owner,format!("#!/bin/sh\nexit {exit}\n"))?;
            let output = invoke(command(),b"").await?;
            assert!(!output.status.success() && output.stdout.is_empty());
        }
        std::fs::write(&owner.owner,owner_script)?;
        std::fs::write(&owner.artifact,b"corrupt invented evidence")?;
        let output = invoke(command(),b"").await?;
        assert!(!output.status.success() && output.stdout.is_empty());
        std::fs::write(&owner.artifact,&bytes)?;
        assert_eq!(snapshot(&db).await?,before);
        // Fault only this owned fixture: a valid but changed presentation must
        // fail its authenticated publication binding, even if it could render.
        let mut corrupt = Companion::decode(receipt.bytes())?;
        let edgeagent_contracts::explanation::Outcome::Generated {proposal} = &mut corrupt.outcome else {return Err("generated".into())};
        proposal.claims[0].variant = Variant::Standard;
        sqlx::raw_sql("DROP TRIGGER edgeagent_explanation_completion_immutable ON edgeagent_explanation_completions").execute(&db.pool).await?;
        sqlx::query("UPDATE edgeagent_explanation_completions SET companion=$1").bind(corrupt.canonical_bytes()?).execute(&db.pool).await?;
        let damaged = snapshot(&db).await?;
        let output = invoke(command(),b"").await?;
        assert!(!output.status.success() && output.stdout.is_empty());
        assert_eq!(snapshot(&db).await?,damaged);
        Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}
fn publication_request(
    event: &edgeagent_contracts::MessageEnvelope,
) -> Result<edgeagent_contracts::research::ResearchRequest> {
    Ok(edgeagent_research_application::publication::evaluation_request(event)?)
}
