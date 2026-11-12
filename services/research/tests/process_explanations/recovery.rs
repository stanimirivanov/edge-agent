//! Observed process death and real relay/duplicate/rebuild boundaries.
use super::*;
use edgeagent_research_application::{explanation_worker::Prepared, publication::decode_decision};
struct Case {
    id: String,
    original: Vec<u8>,
    model: model::Server,
    authority: tokio::process::Child,
    from: i64,
    until: i64,
}
impl Case {
    async fn new(h: &mut Journey) -> Result<Self> {
        h.seed_history().await?;
        let address = address()?;
        let origin = format!("http://{address}");
        h.start_query_servers(None, Some((&origin, AUDIENCE)))
            .await?;
        submit(h, 5).await?;
        h.run("research", "offline-worker", &[]).await?;
        let original = result(h, "artifact").await?;
        let mut scheduler = h.command("research", "offline-explanation-scheduler")?;
        scheduler.env("EDGEAGENT_RESEARCH_CONSUMER", "explanation-jobs");
        output(scheduler).await?;
        let store =
            edgeagent_research_postgres::PostgresResearchStore::new(h.research.pool.clone());
        let job = store
            .explanation_job("operator", None)
            .await?
            .ok_or("job")?;
        let ResearchRecord::Artifact(artifact) = ResearchRecord::decode(&original)? else {
            return Err("artifact".into());
        };
        for purpose in ["model_disclosure", "explanation_serving"] {
            let grant = serde_json::json!({"kind":"research.explanation.grant.v1","anchor":job.job.anchor,
                "audience":AUDIENCE,"purpose":purpose,"document":null,"revision":1,
                "valid_from_ms":artifact.request.decision_at_ms,"expires_at_ms":artifact.expires_at_ms,"revoked":false});
            h.input(
                "research",
                "offline-explanation-grant",
                &serde_json::to_vec(&grant)?,
            )
            .await?;
        }
        let mut config = profile()?;
        config.settings.timeout_ms = 15000;
        std::fs::write(h.root.join("explanation.json"), config.canonical_bytes()?)?;
        std::fs::write(
            h.root.join("synthetic-explanation-proof"),
            b"PR19 invented fixtures only\n",
        )?;
        let decision = decode_decision(job.original.receipt.decision().ok_or("decision")?)?;
        let prepared = Prepared::new(job.original.receipt.record(), &decision, &config)?;
        let model = model::Server::start(
            &h.root,
            prepared.input(),
            Some("valid"),
            &config.deployment.weights,
        )
        .await?;
        let mut server = h.command("research", "offline-explanation-authority")?;
        server
            .env("EDGEAGENT_RESEARCH_EXPLANATION_LISTEN", address.to_string())
            .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE);
        let mut authority = server.spawn()?;
        observe(|| {
            if authority.try_wait()?.is_some() {
                return Err("authority exited".into());
            }
            Ok(std::net::TcpStream::connect(address).is_ok())
        })
        .await?;
        let case = Self {
            id: job.job.id()?,
            original,
            model,
            authority,
            from: artifact.request.decision_at_ms,
            until: artifact.expires_at_ms,
        };
        case.review(h, 1, "allow").await?;
        Ok(case)
    }
    fn command(&self, h: &Journey, service: &str, mode: &str) -> Result<tokio::process::Command> {
        let mut command = h.command(service, mode)?;
        command
            .env("EDGEAGENT_SYNTHETIC_EXPLANATION_PROOF", "1")
            .env("EDGEAGENT_RESEARCH_EXPLANATION_JOB", &self.id)
            .env(
                "EDGEAGENT_RESEARCH_EXPLANATION_PROFILE",
                h.root.join("explanation.json"),
            )
            .env("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE", AUDIENCE);
        Ok(command)
    }
    async fn recover(&self, h: &Journey) -> Result<Vec<u8>> {
        output(self.command(h, "research", "offline-explanation-worker")?).await
    }
    async fn review(&self, h: &Journey, revision: i64, decision: &str) -> Result {
        let review = serde_json::json!({"audience":AUDIENCE,"agreement":ACQUISITION,"revision":revision,"decision":decision,
            "valid_from_ms":self.from,"expires_at_ms":self.until});
        h.input_with_args(
            "market",
            "offline-explanation-review",
            &[ACQUISITION],
            &serde_json::to_vec(&review)?,
        )
        .await?;
        Ok(())
    }
    async fn query(&self, h: &Journey) -> Result<(u16, Vec<u8>)> {
        h.request(
            "GET",
            "/v1/research-requests/operator/qualification/explanation",
            &[],
        )
        .await
    }
    async fn one_terminal(&self, h: &Journey, bytes: &[u8], calls: usize) -> Result {
        assert_eq!(self.recover(h).await?, bytes);
        assert_eq!(self.model.calls(), calls);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_attempts")
                .fetch_one(&h.research.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_explanation_completions")
                .fetch_one(&h.research.pool)
                .await?,
            1
        );
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_message_outbox WHERE message_type='com.edgeagent.research.explanation-recorded.v1'").fetch_one(&h.research.pool).await?,1);
        assert_eq!(
            h.request(
                "GET",
                "/v1/research-requests/operator/qualification/outcome",
                &[]
            )
            .await?,
            (200, self.original.clone())
        );
        Ok(())
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = self.authority.start_kill();
    }
}
async fn observe(mut ready: impl FnMut() -> Result<bool>) -> Result {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !ready()? {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Result::Ok(())
    })
    .await?
}
async fn deadline(h: &Journey) -> Result {
    let until: i64 = sqlx::query_scalar("SELECT deadline_ms FROM edgeagent_explanation_attempts")
        .fetch_one(&h.research.pool)
        .await?;
    observe(|| Ok(chrono::Utc::now().timestamp_millis() >= until)).await
}
async fn kill(mut child: tokio::process::Child) -> Result {
    child.start_kill()?;
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
    assert!(!output.status.success() && output.stdout.is_empty());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and real service binaries via research-processes gate"]
async fn death_at_reservation_request_response_and_commit_recovers_one_terminal() -> Result {
    for stage in ["reserved", "request", "response", "completion", "committed"] {
        let mut h = Journey::new().await?;
        let tested = AssertUnwindSafe(async {
            let case = Case::new(&mut h).await?;
            if stage == "request" {
                case.model.hold();
            }
            let key = format!("{}/proof-completion",h.research.name);
            let mut commit_guard = if stage == "completion" { Some(h.research.pool.acquire().await?) } else { None };
            if let Some(guard) = commit_guard.as_mut() {
                sqlx::query("SELECT pg_advisory_lock(hashtextextended($1,0))").bind(&key).execute(&mut **guard).await?;
                sqlx::raw_sql("CREATE FUNCTION proof_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(hashtextextended(TG_TABLE_SCHEMA || '/proof-completion',0)); RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER proof_completion AFTER INSERT ON edgeagent_explanation_completions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION proof_completion()")
                    .execute(&h.research.pool).await?;
            }
            let mut command = case.command(&h, "qualification", "valid")?;
            command.env("EDGEAGENT_SYNTHETIC_EXPLANATION_BARRIER", stage);
            let mut child = command.spawn()?;
            if stage == "completion" {
                condition(&h,"SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND NOT granted AND ((classid::bigint << 32) | objid::bigint)=hashtextextended($1,0))",&key,10).await?;
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_explanation_completions").fetch_one(&h.research.pool).await?,0);
            } else {
            observe(|| {
                if child.try_wait()?.is_some() {
                    return Err(format!("exited before {stage} barrier").into());
                }
                Ok(if stage == "request" {
                    case.model.ready()
                } else {
                    h.root.join(format!("proof-{stage}")).is_file()
                })
            })
            .await?;
            }
            if matches!(stage,"response"|"completion"|"committed") {
                assert_eq!(std::fs::read(h.root.join("proof-model-result"))?, b"generated");
            }
            let calls = usize::from(stage != "reserved");
            assert_eq!(case.model.calls(), calls);
            kill(child).await?;
            case.model.release();
            if let Some(guard) = commit_guard.as_mut() {
                sqlx::query("SELECT pg_advisory_unlock(hashtextextended($1,0))").bind(&key).execute(&mut **guard).await?;
                // COMMIT may already have reached PostgreSQL: losing the caller
                // cannot guarantee rollback. Wait for the real transaction to settle.
                sqlx::raw_sql("DROP TRIGGER proof_completion ON edgeagent_explanation_completions; DROP FUNCTION proof_completion()")
                    .execute(&h.research.pool).await?;
            }
            drop(commit_guard);
            let first = case.recover(&h).await?;
            let bytes = if first.is_empty() {
                assert!(stage != "committed"); deadline(&h).await?; case.recover(&h).await?
            } else {
                assert!(matches!(stage,"completion"|"committed")); first
            };
            match Companion::decode(&bytes)?.outcome {
                Outcome::Generated { .. } if matches!(stage,"completion"|"committed") => {}
                Outcome::Fallback {
                    reason: Fallback::UncertainAttempt,
                    ..
                } if stage != "committed" => {}
                _ => return Err(format!("wrong recovery at {stage}").into()),
            }
            case.one_terminal(&h, &bytes, calls).await
        })
        .catch_unwind()
        .await;
        h.finish(tested).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and real service binaries via research-processes gate"]
async fn late_model_response_cannot_replace_recovered_fallback() -> Result {
    let mut h = Journey::new().await?;
    let tested = AssertUnwindSafe(async {
        let case = Case::new(&mut h).await?;
        case.model.hold();
        let mut command = case.command(&h, "qualification", "valid")?;
        command.env("EDGEAGENT_SYNTHETIC_EXPLANATION_BARRIER", "response");
        let child = command.spawn()?;
        observe(|| Ok(case.model.ready())).await?;
        deadline(&h).await?;
        let bytes = case.recover(&h).await?;
        assert!(matches!(
            Companion::decode(&bytes)?.outcome,
            Outcome::Fallback {
                reason: Fallback::UncertainAttempt,
                ..
            }
        ));
        let before = retained(&h).await?;
        case.model.release();
        observe(|| Ok(h.root.join("proof-response").is_file())).await?;
        assert_eq!(
            std::fs::read(h.root.join("proof-model-result"))?,
            b"generated"
        );
        std::fs::write(h.root.join("proof-release-response"), b"release")?;
        let late = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await??;
        assert!(!late.status.success() && late.stdout.is_empty());
        assert_eq!(retained(&h).await?, before);
        case.one_terminal(&h, &bytes, 1).await
    })
    .catch_unwind()
    .await;
    h.finish(tested).await
}

async fn condition(h: &Journey, sql: &'static str, key: &str, seconds: u64) -> Result {
    tokio::time::timeout(Duration::from_secs(seconds), async {
        while !sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(sql))
            .bind(key)
            .fetch_one(&h.research.pool)
            .await?
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Result::Ok(())
    })
    .await?
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and real service binaries via research-processes gate"]
async fn relay_death_duplicate_delivery_and_rebuild_preserve_current_authorization() -> Result {
    let mut h = Journey::new().await?;
    let tested=AssertUnwindSafe(async {
        let case=Case::new(&mut h).await?;
        let bytes=output(case.command(&h,"qualification","valid")?).await?;
        let decoded=Companion::decode(&bytes)?;let id=decoded.id()?;
        let key=format!("{}/proof-mark-published",h.research.name);
        let mut guard=h.research.pool.acquire().await?;
        sqlx::query("SELECT pg_advisory_lock(hashtextextended($1,0))").bind(&key).execute(&mut *guard).await?;
        sqlx::raw_sql("CREATE FUNCTION proof_mark_published() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.published_at IS NOT NULL AND OLD.published_at IS NULL THEN PERFORM pg_advisory_xact_lock(hashtextextended(TG_TABLE_SCHEMA || '/proof-mark-published',0)); END IF; RETURN NEW; END $$; CREATE TRIGGER proof_mark_published BEFORE UPDATE ON edgeagent_message_outbox FOR EACH ROW WHEN (NEW.message_type='com.edgeagent.research.explanation-recorded.v1') EXECUTE FUNCTION proof_mark_published()")
            .execute(&h.research.pool).await?;
        let relay=h.command("research","offline-relay")?.spawn()?;
        condition(&h,"SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND NOT granted AND ((classid::bigint << 32) | objid::bigint)=hashtextextended($1,0))",&key,10).await?;
        // Receiving and querying the real broker event proves publish happened
        // while the observed mark-published transaction is still blocked.
        h.run("projector","offline-worker",&[]).await?;
        assert_eq!(case.query(&h).await?,(200,bytes.clone()));
        kill(relay).await?;
        sqlx::query("SELECT pg_advisory_unlock(hashtextextended($1,0))").bind(&key).execute(&mut *guard).await?;
        drop(guard);
        condition(&h,"SELECT published_at IS NULL FROM edgeagent_message_outbox WHERE message_id=$1",&id,5).await?;
        sqlx::raw_sql("DROP TRIGGER proof_mark_published ON edgeagent_message_outbox; DROP FUNCTION proof_mark_published()")
            .execute(&h.research.pool).await?;
        // Observe the actual 30-second relay lease; never edit its timestamp.
        condition(&h,"SELECT lease_expires_at <= clock_timestamp() FROM edgeagent_message_outbox WHERE message_id=$1",&id,35).await?;
        h.run("research","offline-relay",&[]).await?;
        assert!(sqlx::query_scalar::<_,bool>("SELECT published_at IS NOT NULL AND attempt_count=2 FROM edgeagent_message_outbox WHERE message_id=$1").bind(&id).fetch_one(&h.research.pool).await?);
        let publication:Vec<u8>=sqlx::query_scalar("SELECT publication FROM edgeagent_explanation_completions").fetch_one(&h.research.pool).await?;
        // Bypass publish deduplication to exercise an actual duplicate delivery.
        h.context.publish(edgeagent_contracts::explanation::RECORDED.subject()?,publication.into()).await?.await?;
        h.run("projector","offline-worker",&[]).await?;
        assert_eq!(case.query(&h).await?,(200,bytes.clone()));
        case.one_terminal(&h,&bytes,1).await?;
        let checkpoint:Vec<u8>=sqlx::query_scalar("SELECT checkpoint FROM edgeagent_research_views").fetch_one(&h.projector.pool).await?;
        case.review(&h,2,"revoke").await?;
        assert_eq!(case.query(&h).await?,(403,br#"{"status":"forbidden"}"#.to_vec()));
        h.rebuild().await?;
        assert_eq!(case.query(&h).await?.0,404);
        for _ in 0..4 {h.run("projector","offline-worker",&[]).await?;}
        let rebuilt:Vec<u8>=sqlx::query_scalar("SELECT checkpoint FROM edgeagent_research_views").fetch_one(&h.projector.pool).await?;
        assert_eq!(rebuilt,checkpoint);
        assert_eq!(case.query(&h).await?,(403,br#"{"status":"forbidden"}"#.to_vec()));
        case.review(&h,3,"allow").await?;
        assert_eq!(case.query(&h).await?,(200,bytes.clone()));
        case.one_terminal(&h,&bytes,1).await
    }).catch_unwind().await;
    h.finish(tested).await
}
