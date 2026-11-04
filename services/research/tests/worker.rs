//! Real completion and owner-process behavior in unique private PostgreSQL schemas.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated schema inspection/fault injection."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use edgeagent_contracts::research::ResearchRecord;
use futures_util::FutureExt;
use support::governed_event as event;
use support::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn restart_and_lost_result_recover_before_owner_read_with_one_publication() -> Result {
    let db = Database::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let (event, bytes) = event("request-1")?;
        let profile = Profile::new(&bytes)?;
        let first = run(&profile, &db.schema, &event.to_json()?).await?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert!(
            matches!(
                ResearchRecord::decode(&first.stdout)?,
                ResearchRecord::Artifact(_)
            ),
            "synthetic outcome: {}",
            String::from_utf8_lossy(&first.stdout)
        );
        profile.unavailable()?;
        std::fs::remove_file(&profile.owner)?;
        // Lost result and all process state; the second process must not need its failed owner.
        let again = run(&profile, &db.schema, &event.to_json()?).await?;
        assert!(again.status.success());
        assert_eq!(first.stdout, again.stdout);
        for table in [
            "edgeagent_research_completions",
            "edgeagent_research_decisions",
            "edgeagent_message_outbox",
            "edgeagent_message_inbox",
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&db.pool)
                    .await?;
            assert_eq!(
                count,
                if table == "edgeagent_message_outbox" {
                    2
                } else {
                    1
                }
            );
        }
        let publication: Vec<u8> =
            sqlx::query_scalar("SELECT publication FROM edgeagent_research_completions")
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(
            edgeagent_contracts::MessageEnvelope::from_json(&publication)?
                .payload::<ResearchRecord>()?
                .canonical_bytes()?,
            first.stdout
        );
        let record = ResearchRecord::decode(&first.stdout)?;
        let ResearchRecord::Artifact(base) = &record else {
            return Err("artifact".into());
        };
        let lifecycle = edgeagent_contracts::research::ResearchLifecycle {
            scope: base.request.scope.clone(),
            request_id: base.request.request_id.clone(),
            artifact: record.digest()?,
            revision: 1,
            effective_at_ms: base.expires_at_ms,
            change: edgeagent_contracts::research::LifecycleChange::Expired,
        };
        let bytes = ResearchRecord::Lifecycle(lifecycle).canonical_bytes()?;
        let mut previous = None;
        for _ in 0..2 {
            use tokio::io::AsyncWriteExt as _;
            let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-research"))
                .arg("offline-lifecycle")
                .env(
                    "EDGEAGENT_POSTGRES_URL",
                    std::env::var("EDGEAGENT_POSTGRES_URL")?,
                )
                .env("EDGEAGENT_RESEARCH_SCHEMA", &db.schema)
                .env("EDGEAGENT_RESEARCH_SCOPE", &base.request.scope)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            child.stdin.take().ok_or("stdin")?.write_all(&bytes).await?;
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                    .await??;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let envelope = edgeagent_contracts::MessageEnvelope::from_json(&output.stdout)?;
            edgeagent_research_application::lifecycle::decode(&envelope)?;
            if let Some(old) = previous {
                assert_eq!(output.stdout, old);
            }
            previous = Some(output.stdout);
        }
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn corrupt_and_denied_reads_abstain_while_unavailable_reads_leave_no_completion() -> Result {
    let db = Database::new().await?;
    let result=std::panic::AssertUnwindSafe(async {
        let (event,bytes)=event("corrupt")?;let profile=Profile::new(&bytes)?;
        std::fs::write(&profile.artifact,b"corrupt invented bytes")?;
        let output=run(&profile,&db.schema,&event.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::InvalidEvidence));
        let (denied,_)=support::governed_event("denied")?;std::fs::write(&profile.owner,"#!/bin/sh\nexit 77\n")?;
        let output=run(&profile,&db.schema,&denied.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::MissingEntitlement));
        let mut legacy:serde_json::Value=serde_json::from_slice(&denied.to_json()?)?;
        let mut request=edgeagent_research_application::publication::evaluation_request(&denied)?;request.request_id="legacy".into();
        let record=ResearchRecord::Request(request.clone());let (key,partition)=edgeagent_contracts::research_acquisition::binding(&request)?;
        legacy["type"]=edgeagent_research_application::publication::EVALUATE.message_type().into();legacy["source"]=edgeagent_contracts::Component::Gateway.source_uri().into();legacy["dataschema"]=edgeagent_research_application::publication::EVALUATE.data_schema().into();legacy["data"]=serde_json::to_value(record)?;
        legacy["id"]=key.clone().into();legacy["correlationid"]=key.clone().into();legacy["idempotencykey"]=key.into();legacy["subject"]=partition.clone().into();legacy["partitionkey"]=partition.into();profile.unavailable()?;
        let output=run(&profile,&db.schema,&serde_json::to_vec(&legacy)?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::MissingEntitlement));
        std::fs::write(&profile.artifact,vec![b' ';edgeagent_contracts::evidence::MAX_EVIDENCE_BYTES+1])?;
        std::fs::write(&profile.owner,format!("#!/bin/sh\n/bin/cat '{}'\n",profile.artifact.display()))?;
        let (oversized,_)=support::governed_event("oversized")?;
        let output=run(&profile,&db.schema,&oversized.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::InvalidEvidence));
        let (unavailable,_)=support::governed_event("unavailable")?;profile.unavailable()?;
        assert!(!run(&profile,&db.schema,&unavailable.to_json()?).await?.status.success());
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_completions").fetch_one(&db.pool).await?,4);Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn governed_processes_require_owner_approval_and_retirement_preserves_original_receipt()
-> Result {
    async fn command(
        profile: &Profile,
        schema: &str,
        mode: &str,
        bytes: &[u8],
    ) -> Result<std::process::Output> {
        use tokio::io::AsyncWriteExt as _;
        let mut cmd = profile.command(schema, mode);
        cmd.env("EDGEAGENT_RESEARCH_LOOKBACK_SESSIONS", "1");
        let mut child = tokio::process::Command::from(cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        child.stdin.take().ok_or("stdin")?.write_all(bytes).await?;
        Ok(
            tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                .await??,
        )
    }
    let db = Database::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let (event, bytes) = event("governed-request")?;
        let profile = Profile::new(&bytes)?;
        let binding = support::governance_profile()?.bytes()?;
        let input = event.to_json()?;
        assert!(
            !command(&profile, &db.schema, "offline-governed-once", &input)
                .await?
                .status
                .success()
        );
        for mode in ["offline-governance-install", "offline-governance-install"] {
            assert!(
                command(&profile, &db.schema, mode, &binding)
                    .await?
                    .status
                    .success()
            );
        }
        assert!(
            !command(&profile, &db.schema, "offline-governed-once", &input)
                .await?
                .status
                .success()
        );
        assert!(
            command(&profile, &db.schema, "offline-governance-approve", &binding)
                .await?
                .status
                .success()
        );
        let first = command(&profile, &db.schema, "offline-governed-once", &input).await?;
        assert!(first.status.success());
        assert!(
            matches!(
                ResearchRecord::decode(&first.stdout)?,
                ResearchRecord::Artifact(_)
            ),
            "synthetic outcome: {}",
            String::from_utf8_lossy(&first.stdout)
        );
        let original_owner = std::fs::read(&profile.owner)?;
        profile.unavailable()?;
        for (scope, expected) in [("foreign", false), ("operator", true)] {
            let control = edgeagent_contracts::research_controls::Command::V1 {
                scope: scope.into(),
                target: edgeagent_contracts::research_controls::Target::Scope,
                revision: 1,
                enabled: false,
                reason_code: "maintenance".into(),
            }
            .bytes()?;
            assert_eq!(
                command(&profile, &db.schema, "offline-control", &control)
                    .await?
                    .status
                    .success(),
                expected
            );
        }
        let recovered = command(&profile, &db.schema, "offline-governed-once", &input).await?;
        assert!(recovered.status.success());
        assert_eq!(recovered.stdout, first.stdout);
        let (blocked, _) = support::governed_event("kill-blocked")?;
        assert!(
            !command(
                &profile,
                &db.schema,
                "offline-governed-once",
                &blocked.to_json()?
            )
            .await?
            .status
            .success()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_completions")
                .fetch_one(&db.pool)
                .await?,
            1
        );
        let resume = edgeagent_contracts::research_controls::Command::V1 {
            scope: "operator".into(),
            target: edgeagent_contracts::research_controls::Target::Scope,
            revision: 2,
            enabled: true,
            reason_code: "reviewed-resume".into(),
        }
        .bytes()?;
        assert!(
            command(&profile, &db.schema, "offline-control", &resume)
                .await?
                .status
                .success()
        );
        std::fs::write(&profile.owner, original_owner)?;
        assert!(
            command(&profile, &db.schema, "offline-governance-retire", &binding)
                .await?
                .status
                .success()
        );
        profile.unavailable()?;
        let recovered = command(&profile, &db.schema, "offline-governed-once", &input).await?;
        assert!(recovered.status.success());
        assert_eq!(recovered.stdout, first.stdout);
        let (next, _) = support::governed_event("governed-request-2")?;
        assert!(
            !command(
                &profile,
                &db.schema,
                "offline-governed-once",
                &next.to_json()?
            )
            .await?
            .status
            .success()
        );
        assert!(
            !command(&profile, &db.schema, "offline-governance-approve", &binding)
                .await?
                .status
                .success()
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn configured_expiry_cli_recovers_exact_event_after_restart_without_owner_or_configuration()
-> Result {
    let db = Database::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let mut wire = execution()?;
        let edgeagent_contracts::strategy::ExecutionConfiguration::V1 { bundle, .. } = &mut wire;
        let edgeagent_contracts::strategy::Configuration::V1 { strategy, .. } = bundle;
        strategy.invalidation_revision =
            edgeagent_research_application::execution::CONFIGURED_INVALIDATION_REVISION.into();
        let selection =
            edgeagent_research_application::execution::Selection::decode(&wire.canonical_bytes()?)?;
        let binding = selection.profile("operator");
        let (delivery, bytes) =
            event_with_configuration("configured-expiry", binding.configuration.clone())?;
        let profile = Profile::new(&bytes)?;
        std::fs::write(profile.root.join("execution.json"), wire.canonical_bytes()?)?;
        let store = edgeagent_research_postgres::PostgresResearchStore::new(db.pool.clone());
        store.govern(&binding, 1).await?;
        store.govern(&binding, 2).await?;
        async fn invoke(
            mut command: std::process::Command,
            input: &[u8],
        ) -> Result<std::process::Output> {
            use tokio::io::AsyncWriteExt as _;
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = tokio::process::Command::from(command)
                .kill_on_drop(true)
                .spawn()?;
            child.stdin.take().ok_or("stdin")?.write_all(input).await?;
            Ok(
                tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                    .await??,
            )
        }
        let mut command = profile.command(&db.schema, "offline-once");
        command.env("EDGEAGENT_RESEARCH_CONFIGURATION", &binding.configuration);
        let first = invoke(command, &delivery.to_json()?).await?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let ResearchRecord::Artifact(base) = ResearchRecord::decode(&first.stdout)? else {
            return Err("artifact".into());
        };
        let check = edgeagent_contracts::research_invalidation::Command::V1 {
            original: base.request,
            effective_at_ms: base.expires_at_ms,
            reassessment: None,
        };
        // Configuration and evidence adapters are not needed for a retained owner check.
        std::fs::remove_file(profile.root.join("execution.json"))?;
        profile.unavailable()?;
        let mut previous = None;
        for _ in 0..2 {
            let output = invoke(
                profile.command(&db.schema, "offline-invalidation-check"),
                &check.bytes()?,
            )
            .await?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let envelope = edgeagent_contracts::MessageEnvelope::from_json(&output.stdout)?;
            assert!(matches!(
                edgeagent_research_application::lifecycle::decode(&envelope)?.change,
                edgeagent_contracts::research::LifecycleChange::Expired
            ));
            if let Some(old) = previous {
                assert_eq!(output.stdout, old);
            }
            previous = Some(output.stdout);
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_lifecycle")
                .fetch_one(&db.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_outbox")
                .fetch_one(&db.pool)
                .await?,
            3
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
