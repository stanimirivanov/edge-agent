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
