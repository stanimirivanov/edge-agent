//! Four real services and real owner evidence; no fabricated calculation or query output.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned schema inspection, observed lock barrier and disposable projection rebuild."
)]
mod process_support;
#[path = "process_telemetry/mod.rs"]
mod telemetry;
use edgeagent_contracts::{
    MessageEnvelope,
    research::{AbstentionReason, LifecycleChange, ResearchLifecycle, ResearchRecord},
};
use futures_util::FutureExt;
use process_support::*;
use std::{panic::AssertUnwindSafe, time::Duration};
async fn submit(h: &Journey, lookback: u16) -> Result {
    submit_intent(h, &h.intent(lookback)?).await
}
async fn submit_intent(
    h: &Journey,
    intent: &edgeagent_contracts::research::ResearchRequest,
) -> Result {
    let request = serde_json::to_vec(intent)?;
    let first = h.request("POST", "/v1/research-requests", &request).await?;
    assert_eq!(first.0, 202);
    assert_eq!(
        h.request("POST", "/v1/research-requests", &request).await?,
        first
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_gateway_requests")
            .fetch_one(&h.gateway.pool)
            .await?,
        1
    );
    assert_eq!(
        h.request("GET", "/v1/research-requests/operator/qualification", &[])
            .await?
            .0,
        404
    );
    h.gateway_relay().await?;
    h.consume_market().await?;
    Ok(())
}
async fn result(h: &Journey, status: &str) -> Result<Vec<u8>> {
    h.run("research", "offline-relay", &[]).await?;
    h.run("projector", "offline-worker", &[]).await?;
    h.run("projector", "offline-worker", &[]).await?;
    let response = h
        .request(
            "GET",
            "/v1/research-requests/operator/qualification/outcome",
            &[],
        )
        .await?;
    assert_eq!(response.0, 200);
    let original: Vec<u8> =
        sqlx::query_scalar("SELECT outcome FROM edgeagent_research_completions")
            .fetch_one(&h.research.pool)
            .await?;
    assert_eq!(response.1, original);
    let metadata = h
        .request("GET", "/v1/research-requests/operator/qualification", &[])
        .await?;
    assert_eq!(metadata.0, 200);
    let value: serde_json::Value = serde_json::from_slice(&metadata.1)?;
    assert_eq!(value["status"], status);
    Ok(original)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and four freshly built service binaries via research-processes gate"]
async fn four_process_artifact_survives_death_redelivery_rebuild_and_lifecycle() -> Result {
    let mut h = Journey::new().await?;
    let tested=AssertUnwindSafe(async {
        h.seed_history().await?;h.start_servers().await?;submit(&h,5).await?;
        let mut barrier=h.research.pool.begin().await?;
        sqlx::raw_sql("LOCK TABLE edgeagent_research_completions IN SHARE MODE").execute(&mut *barrier).await?;
        let mut worker=h.command("research","offline-worker")?.spawn()?;
        let table=format!("{}.edgeagent_research_completions",h.research.name);
        tokio::time::timeout(Duration::from_secs(10),async {loop {
            if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE relation=$1::regclass AND NOT granted)").bind(&table).fetch_one(&h.projector.pool).await? {break;}
            tokio::time::sleep(Duration::from_millis(25)).await;
        }Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())}).await??;
        worker.start_kill()?;tokio::time::timeout(Duration::from_secs(5),worker.wait()).await??;
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_completions").fetch_one(&mut *barrier).await?,0);
        barrier.rollback().await?;
        // Retain the original unacknowledged message. Start the replacement late
        // enough that its bounded 30-second receive spans the real 60-second ACK wait.
        tokio::time::sleep(Duration::from_secs(35)).await;
        h.run("research","offline-worker",&[]).await?;
        let stream=h.context.get_stream(&h.stream).await?;
        let consumer:async_nats::jetstream::consumer::PullConsumer=stream.get_consumer("research").await?;
        assert_eq!(consumer.cached_info().num_ack_pending,0);
        assert_eq!(consumer.cached_info().delivered.consumer_sequence,2);
        let original=result(&h,"artifact").await?;
        let ResearchRecord::Artifact(artifact)=ResearchRecord::decode(&original)? else {return Err("artifact required".into());};
        assert_eq!(artifact.ranking.len(),1);assert_eq!(artifact.ranking[0].score_ppm,40502);
        let input:Vec<u8>=sqlx::query_scalar("SELECT envelope FROM edgeagent_message_inbox WHERE consumer_name='research-completion'").fetch_one(&h.research.pool).await?;
        assert_eq!(h.input("research","offline-replay",&input).await?,original);
        // Actual duplicate event, bypassing broker publish deduplication while retaining identity.
        let ready=MessageEnvelope::from_json(&input)?;
        h.context.publish(edgeagent_contracts::research_acquisition::DECIDED.subject()?,input.into()).await?.await?;
        h.run("research","offline-worker",&[]).await?;
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_completions").fetch_one(&h.research.pool).await?,1);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_message_outbox").fetch_one(&h.research.pool).await?,2);
        assert_eq!(ready.message_type(),edgeagent_contracts::research_acquisition::DECIDED.message_type());
        h.rebuild().await?;
        assert_eq!(h.request("GET","/v1/research-requests/operator/qualification/outcome",&[]).await?.0,404);
        h.run("projector","offline-worker",&[]).await?;
        h.run("projector","offline-worker",&[]).await?;
        assert_eq!(h.request("GET","/v1/research-requests/operator/qualification/outcome",&[]).await?.1,original);
        let event=ResearchRecord::Lifecycle(ResearchLifecycle{scope:"operator".into(),request_id:"qualification".into(),artifact:ResearchRecord::Artifact(artifact.clone()).digest()?,revision:1,effective_at_ms:artifact.expires_at_ms,change:LifecycleChange::Expired});
        h.input("research","offline-lifecycle",&event.canonical_bytes()?).await?;
        h.run("research","offline-relay",&[]).await?;h.run("projector","offline-worker",&[]).await?;
        assert_eq!(h.request("GET","/v1/research-requests/operator/qualification/outcome",&[]).await?.0,410);
        assert_eq!(sqlx::query_scalar::<_,Vec<u8>>("SELECT outcome FROM edgeagent_research_completions").fetch_one(&h.research.pool).await?,original);
        Ok(())
    }).catch_unwind().await;
    h.finish(tested).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and four freshly built service binaries via research-processes gate"]
async fn four_process_owner_history_shortfall_is_a_stable_abstention() -> Result {
    let mut h = Journey::new().await?;
    let tested=AssertUnwindSafe(async {
        h.seed_history().await?;h.start_servers().await?;submit(&h,6).await?;
        h.run("research","offline-worker",&[]).await?;
        let bytes=result(&h,"abstained").await?;
        assert!(matches!(ResearchRecord::decode(&bytes)?,ResearchRecord::Abstention(a) if a.reason==AbstentionReason::InsufficientHistory));
        Ok(())
    }).catch_unwind().await;
    h.finish(tested).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and four freshly built service binaries via research-processes gate"]
async fn four_process_m05_promotion_policy_replay_and_configured_expiry() -> Result {
    for deny in [false, true] {
        let mut h = Journey::new().await?;
        let tested = AssertUnwindSafe(qualify_m05(&mut h, deny))
            .catch_unwind()
            .await;
        h.finish(tested).await?;
    }
    Ok(())
}

async fn qualify_m05(h: &mut Journey, deny: bool) -> Result {
    use edgeagent_contracts::{
        research_controls::{Command as Control, Target},
        research_decision::{Assessment, DecisionProvenance, Reason, Status},
        research_invalidation::Command as Invalidation,
        strategy::{Configuration, ExecutionConfiguration},
    };
    use edgeagent_research_application::execution::{CONFIGURED_INVALIDATION_REVISION, Selection};

    h.seed_history().await?;
    h.start_servers().await?;
    let mut configuration = execution(5)?;
    let ExecutionConfiguration::V1 { bundle, .. } = &mut configuration;
    let Configuration::V1 {
        strategy, policy, ..
    } = bundle;
    strategy.invalidation_revision = CONFIGURED_INVALIDATION_REVISION.into();
    if deny {
        policy.min_daily_turnover_cents = u64::MAX;
    }
    let request = h.intent_with(5, &configuration)?;
    let profile = Selection::decode(&configuration.canonical_bytes()?)?.profile(&request.scope);
    submit_intent(h, &request).await?;
    let stream = h.context.get_stream(&h.stream).await?;
    let ready = stream
        .get_last_raw_message_by_subject(
            &edgeagent_contracts::research_acquisition::DECIDED.subject()?,
        )
        .await?;
    let delivery = MessageEnvelope::from_json(&ready.payload)?.to_json()?;

    let profile_bytes = profile.bytes()?;
    h.input("research", "offline-governance-install", &profile_bytes)
        .await?;
    assert!(
        h.input("research", "offline-governance-approve", &profile_bytes)
            .await
            .is_err()
    );
    assert!(
        h.input("research", "offline-once", &delivery)
            .await
            .is_err()
    );

    let (promotion, commands) = promotion_fixture::promotion(&profile)?;
    for command in &commands {
        let bytes = command.bytes()?;
        assert_eq!(
            h.input("research", "offline-experiment-ledger", &bytes)
                .await?,
            bytes
        );
    }
    h.input(
        "research",
        "offline-governance-approve",
        &promotion.bytes()?,
    )
    .await?;
    for (revision, enabled) in [(1, false), (2, true)] {
        let control = Control::V1 {
            scope: request.scope.clone(),
            target: Target::Scope,
            revision,
            enabled,
            reason_code: "qualification".into(),
        };
        h.input("research", "offline-control", &control.bytes()?)
            .await?;
        if !enabled {
            assert!(
                h.input("research", "offline-once", &delivery)
                    .await
                    .is_err()
            );
            for table in [
                "edgeagent_research_completions",
                "edgeagent_research_decisions",
                "edgeagent_message_inbox",
                "edgeagent_message_outbox",
            ] {
                let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {table}"
                )))
                .fetch_one(&h.research.pool)
                .await?;
                assert_eq!(count, 0, "{table}");
            }
        }
    }

    h.run("research", "offline-worker", &[]).await?;
    let original = result(h, if deny { "abstained" } else { "artifact" }).await?;
    let (decision, revision): (Vec<u8>, Option<i64>) =
        sqlx::query_as("SELECT decision,scope_control_revision FROM edgeagent_research_decisions")
            .fetch_one(&h.research.pool)
            .await?;
    assert_eq!(revision, Some(2));
    assert_eq!(h.projected_decision().await?, (200, decision.clone()));
    let retained = DecisionProvenance::decode(&decision)?;
    assert!(retained.request == request);
    assert_eq!(retained.approval_profile, profile.digest()?);
    let metadata = h
        .request("GET", "/v1/research-requests/operator/qualification", &[])
        .await?;
    assert_eq!(metadata.0, 200);
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.1)?;
    assert_eq!(metadata["decision"], retained.digest()?);
    let terminal = ResearchRecord::decode(&original)?;
    if deny {
        assert!(
            matches!(&terminal, ResearchRecord::Abstention(a) if a.reason == AbstentionReason::NoEligibleCandidate)
        );
        assert!(
            matches!(&retained.assessment, Assessment::Evaluated { status: Status::Abstained, eligible, reasons }
            if eligible.is_empty() && reasons == &[Reason::LiquidityBelowMinimum])
        );
    } else {
        assert!(
            matches!(&retained.assessment, Assessment::Evaluated { status: Status::Approved, eligible, reasons }
            if eligible == &request.instruments && reasons.is_empty())
        );
    }

    // Replace the active selection with a different profile before replaying historical bytes.
    h.intent(6)?;
    assert_eq!(
        h.input("research", "offline-replay", &delivery).await?,
        original
    );
    if let ResearchRecord::Artifact(artifact) = terminal {
        let command = Invalidation::V1 {
            original: request,
            effective_at_ms: artifact.expires_at_ms,
            reassessment: None,
        };
        let bytes = command.bytes()?;
        let event = h
            .input("research", "offline-invalidation-check", &bytes)
            .await?;
        assert_eq!(
            h.input("research", "offline-invalidation-check", &bytes)
                .await?,
            event
        );
        h.run("research", "offline-relay", &[]).await?;
        h.run("projector", "offline-worker", &[]).await?;
        assert_eq!(
            h.request(
                "GET",
                "/v1/research-requests/operator/qualification/outcome",
                &[]
            )
            .await?
            .0,
            410
        );
        assert_eq!(h.projected_decision().await?, (200, decision));
    }
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>("SELECT outcome FROM edgeagent_research_completions")
            .fetch_one(&h.research.pool)
            .await?,
        original
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and four freshly built service binaries via research-processes gate"]
async fn explanation_authority_rechecks_marketdata_and_research_on_every_read() -> Result {
    use edgeagent_contracts::explanation::{Anchor, Binding, Companion, Fallback};
    use edgeagent_research_application::{
        explanation::Allowlist,
        explanation_context::{Grant, Purpose, Request},
        publication::decode_decision,
    };
    use edgeagent_research_postgres::{PostgresResearchStore, explanation_completion::Completion};
    const ACQUISITION: &str = "00000000000000000000000000000001";
    const AUDIENCE: &str = "00000000000000000000000000000002";
    let mut h = Journey::new().await?;
    let tested = AssertUnwindSafe(async {
        h.seed_history().await?;
        let authority_address = address()?;
        let origin = format!("http://{authority_address}");
        h.start_query_servers(None, Some((&origin, AUDIENCE))).await?;
        submit(&h, 5).await?;
        h.run("research", "offline-worker", &[]).await?;
        let outcome: Vec<u8> = sqlx::query_scalar("SELECT outcome FROM edgeagent_research_completions")
            .fetch_one(&h.research.pool).await?;
        let record = ResearchRecord::decode(&outcome)?;
        let ResearchRecord::Artifact(artifact) = &record else {
            return Err("expected a Research artifact".into());
        };
        let event: Vec<u8> = sqlx::query_scalar("SELECT publication FROM edgeagent_research_decisions")
            .fetch_one(&h.research.pool).await?;
        let decision = decode_decision(&MessageEnvelope::from_json(&event)?)?;
        let anchor = Anchor {
            scope: artifact.request.scope.clone(),
            artifact: record.digest()?,
            decision: decision.digest()?,
        };
        let grant = Grant {
            request: Request { anchor: anchor.clone(), audience: AUDIENCE.into(), purpose: Purpose::ExplanationServing, document: None },
            valid_from_ms: artifact.request.decision_at_ms,
            expires_at_ms: artifact.expires_at_ms,
            revoked: false,
        };
        let store = PostgresResearchStore::new(h.research.pool.clone());
        store.set_explanation_grant(&grant, 1).await?;
        let validated = Allowlist::from_verified(&record, &decision)?.standard()?;
        let receipt = store.complete_explanation(
            &artifact.request,
            &Binding { anchor, profile: "a".repeat(64), evidence: "b".repeat(64) },
            Completion::Fallback { explanation: &validated, reason: Fallback::Disabled },
            Some(&grant),
            || artifact.request.decision_at_ms,
            "2026-10-30T00:00:00Z",
        ).await?;
        let companion = Companion::decode(receipt.bytes())?.id()?;
        let invalid_listener = h.command("research", "offline-explanation-authority")?
            .env("EDGEAGENT_RESEARCH_EXPLANATION_LISTEN", "0.0.0.0:0")
            .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE)
            .output();
        let invalid_listener = tokio::time::timeout(Duration::from_secs(5), invalid_listener).await??;
        assert!(!invalid_listener.status.success());
        assert!(invalid_listener.stdout.is_empty());
        let address = authority_address;
        let mut server = h.command("research", "offline-explanation-authority")?
            .env("EDGEAGENT_RESEARCH_EXPLANATION_LISTEN", address.to_string())
            .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE)
            .spawn()?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if tokio::net::TcpStream::connect(address).await.is_ok() {
                    return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
                if server.try_wait()?.is_some() {
                    return Err("authorization server exited before listening".into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }).await??;
        let query = serde_json::to_vec(&serde_json::json!({
            "scope": "operator", "request_id": artifact.request.request_id, "audience": AUDIENCE, "companion": companion,
        }))?;
        let check = || Journey::request_at(address, "POST", "/internal/v1/explanation-authorizations", &query);
        assert_eq!(check().await?, (403, Vec::new()));
        let explanation_path = "/internal/v1/research-requests/operator/qualification/explanation";
        let explanation = || Journey::request_at(h.projector_address, "GET", explanation_path, &[]);
        let gateway_explanation = || h.request("GET", "/v1/research-requests/operator/qualification/explanation", &[]);

        assert_eq!(explanation().await?, (404, br#"{"status":"not_observed"}"#.to_vec()));
        h.run("research", "offline-relay", &[]).await?;
        for _ in 0..2 {
            h.run("projector", "offline-worker", &[]).await?;
        }
        assert_eq!(explanation().await?, (404, br#"{"status":"not_observed"}"#.to_vec()));
        h.run("projector", "offline-worker", &[]).await?;
        let status = h.request("GET", "/v1/research-requests/operator/qualification", &[]).await?;
        assert_eq!(status.0, 200);
        let status: serde_json::Value = serde_json::from_slice(&status.1)?;
        assert_eq!(status["projection_revision"], 2);
        assert_eq!(explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        let owner_review = |revision, decision: &str| serde_json::json!({
            "audience": AUDIENCE, "agreement": ACQUISITION, "revision": revision, "decision": decision,
            "valid_from_ms": grant.valid_from_ms, "expires_at_ms": grant.expires_at_ms,
        }).to_string();
        h.input_with_args("market", "offline-explanation-review", &[ACQUISITION], owner_review(1, "allow").as_bytes()).await?;
        assert_eq!(check().await?, (204, Vec::new()));
        assert_eq!(explanation().await?, (200, receipt.bytes().to_vec()));
        assert_eq!(gateway_explanation().await?, (200, receipt.bytes().to_vec()));
        let lock = std::fs::OpenOptions::new().read(true).write(true)
            .open(h.root.join("offline.lock"))?;
        lock.try_lock()?;
        assert_eq!(check().await?, (503, Vec::new()));
        assert_eq!(explanation().await?, (503, br#"{"status":"unavailable"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (503, br#"{"status":"unavailable"}"#.to_vec()));
        drop(lock);

        h.input_with_args("market", "offline-explanation-review", &[ACQUISITION], owner_review(2, "revoke").as_bytes()).await?;
        assert_eq!(check().await?, (403, Vec::new()));
        assert_eq!(explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        h.input_with_args("market", "offline-explanation-review", &[ACQUISITION], owner_review(3, "allow").as_bytes()).await?;
        store.set_explanation_grant(&Grant { revoked: true, ..grant.clone() }, 2).await?;
        assert_eq!(check().await?, (403, Vec::new()));
        assert_eq!(explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (403, br#"{"status":"forbidden"}"#.to_vec()));
        store.set_explanation_grant(&grant, 3).await?;
        assert_eq!(explanation().await?, (200, receipt.bytes().to_vec()));
        assert_eq!(gateway_explanation().await?, (200, receipt.bytes().to_vec()));
        sqlx::query("DELETE FROM edgeagent_message_inbox WHERE message_id=$1")
            .bind(&companion).execute(&h.projector.pool).await?;
        assert_eq!(explanation().await?, (503, br#"{"status":"unavailable"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (503, br#"{"status":"unavailable"}"#.to_vec()));
        store.append_lifecycle("operator", ResearchLifecycle {
            scope: "operator".into(), request_id: artifact.request.request_id.clone(),
            artifact: record.digest()?, revision: 1,
            effective_at_ms: artifact.request.decision_at_ms,
            change: LifecycleChange::Withdrawn,
        }, "2026-10-30T00:00:00Z").await?;
        h.run("research", "offline-relay", &[]).await?;
        h.run("projector", "offline-worker", &[]).await?;
        assert_eq!(explanation().await?, (410, br#"{"status":"withdrawn"}"#.to_vec()));
        assert_eq!(gateway_explanation().await?, (410, br#"{"status":"withdrawn"}"#.to_vec()));
        let (code, status) = h.request("GET", "/v1/research-requests/operator/qualification", &[]).await?;
        assert_eq!(code, 200);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&status)?["projection_revision"], 3);
        let pid = server.id().ok_or("authorization server already stopped")?.to_string();
        assert!(tokio::process::Command::new("kill").args(["-TERM", &pid]).status().await?.success());
        assert!(tokio::time::timeout(Duration::from_secs(5), server.wait()).await??.success());
        Ok(())
    }).catch_unwind().await;
    h.finish(tested).await
}
