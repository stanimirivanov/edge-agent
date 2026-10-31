use super::*;
use edgeagent_contracts::research_controls::{Command, Target};
fn change(target: Target, revision: u64, enabled: bool) -> Command {
    Command::V1 {
        scope: "scope".into(),
        target,
        revision,
        enabled,
        reason_code: if enabled {
            "reviewed-resume"
        } else {
            "maintenance"
        }
        .into(),
    }
}
fn implementation() -> Target {
    Target::Implementation {
        digest: "02".repeat(32),
    }
}
async fn waiting(h: &Harness, pid: i32) -> Result {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND pid=$1)",
            )
            .bind(pid)
            .fetch_one(&h.pool)
            .await?;
            if blocked {
                return Result::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn controls_compose_audit_revisions_and_preserve_recovery_and_lifecycle() -> Result {
    let h = Harness::approved().await?;
    let result=AssertUnwindSafe(async {
        let store=PostgresResearchStore::new(h.pool.clone());
        let first=prepared("scope","first","first",110,3)?;
        let original=store.commit(&first).await?;
        let revisions:(Option<i64>,Option<i64>)=sqlx::query_as("SELECT scope_control_revision,implementation_control_revision FROM edgeagent_research_decisions WHERE request_id='first'").fetch_one(&h.pool).await?;
        assert_eq!(revisions,(Some(0),Some(0)));
        let disable=change(Target::Scope,1,false);
        store.control(&disable).await?;store.control(&disable).await?;
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_controls").fetch_one(&h.pool).await?,1);
        let next=prepared("scope","next","next",110,3)?;
        let scoped=PostgresResearchStore::governed(h.pool.clone(),execution(3)?.profile("scope"));
        assert_eq!(scoped.recover(next.delivery()).await.err().ok_or("pre-read fence")?.kind(),Kind::Disabled);
        assert_eq!(store.commit(&next).await.err().ok_or("commit fence")?.kind(),Kind::Disabled);
        assert_eq!(store.commit(&abstained("scope","blocked-abstention","negative")?).await.err().ok_or("abstention fence")?.kind(),Kind::Disabled);
        assert_eq!(store.recover(first.delivery()).await?.ok_or("receipt")?.bytes(),original.bytes());
        assert_eq!(h.counts().await?,(1,1,1));
        store.commit(&prepared("foreign","independent","foreign",110,3)?).await?;
        store.control(&change(implementation(),1,true)).await?;
        assert_eq!(store.commit(&next).await.err().ok_or("scope precedence")?.kind(),Kind::Disabled);
        store.control(&change(Target::Scope,2,true)).await?;
        assert_eq!(store.control(&disable).await.err().ok_or("stale action")?.kind(),Kind::Conflict);
        assert_eq!(store.control(&change(Target::Scope,2,false)).await.err().ok_or("conflict")?.kind(),Kind::Conflict);
        assert_eq!(store.control(&change(Target::Scope,4,true)).await.err().ok_or("skipped revision")?.kind(),Kind::Conflict);
        let accepted=store.commit(&next).await?;
        let revisions:(Option<i64>,Option<i64>)=sqlx::query_as("SELECT scope_control_revision,implementation_control_revision FROM edgeagent_research_decisions WHERE request_id='next'").fetch_one(&h.pool).await?;
        assert_eq!(revisions,(Some(2),Some(1)));
        store.control(&change(implementation(),2,false)).await?;
        assert_eq!(store.commit(&prepared("scope","other-config","other",110,9)?).await.err().ok_or("implementation fence")?.kind(),Kind::Disabled);
        assert_eq!(store.recover(next.delivery()).await?.ok_or("old receipt")?.bytes(),accepted.bytes());
        store.append_lifecycle("scope",life(&accepted,1,edgeagent_contracts::research::LifecycleChange::Withdrawn)?,TIME).await?;
        let audited:bool=sqlx::query_scalar("SELECT bool_and(actor=session_user AND occurred_at IS NOT NULL) FROM edgeagent_research_controls").fetch_one(&h.pool).await?;
        assert!(audited);
        assert!(sqlx::query("DELETE FROM edgeagent_research_controls").execute(&h.pool).await.is_err());
        // A missing historical enabling action cannot be replaced by current state or a checksum.
        sqlx::raw_sql("ALTER TABLE edgeagent_research_controls DISABLE TRIGGER edgeagent_research_controls_immutable; DELETE FROM edgeagent_research_controls WHERE implementation='' AND revision=2").execute(&h.pool).await?;
        assert_eq!(store.recover(next.delivery()).await.err().ok_or("lost audit proof")?.kind(),Kind::Invalid);
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn control_races_and_failures_fence_completion_and_reject_unfenced_writers() -> Result {
    let h = Harness::approved().await?;
    let result=AssertUnwindSafe(async {
        let store=PostgresResearchStore::new(h.pool.clone());
        let mut barrier=h.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('scope',0))").execute(&mut *barrier).await?;
        let pool=h.reconnect().await?;let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&pool).await?;
        let disable=tokio::spawn(async move {PostgresResearchStore::new(pool).control(&change(Target::Scope,1,false)).await});
        waiting(&h,pid).await?;
        let pool=h.reconnect().await?;let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&pool).await?;
        let p=prepared("scope","blocked","blocked",110,3)?;
        let completion=tokio::spawn(async move {PostgresResearchStore::new(pool).commit(&p).await});
        waiting(&h,pid).await?;barrier.rollback().await?;disable.await??;
        assert_eq!(completion.await?.err().ok_or("disable lost race")?.kind(),Kind::Disabled);assert_eq!(h.counts().await?,(0,0,0));
        store.control(&change(Target::Scope,2,true)).await?;
        let mut barrier=h.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(h.key).execute(&mut *barrier).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION control_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({}); RETURN NEW; END; $$; CREATE TRIGGER control_barrier BEFORE INSERT ON edgeagent_research_decisions FOR EACH ROW EXECUTE FUNCTION control_barrier()",h.key))).execute(&h.pool).await?;
        let pool=h.reconnect().await?;let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&pool).await?;
        let p=prepared("scope","winner","winner",110,3)?;
        let completion=tokio::spawn(async move {PostgresResearchStore::new(pool).commit(&p).await});
        waiting(&h,pid).await?;
        let pool=h.reconnect().await?;let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&pool).await?;
        let disable=tokio::spawn(async move {PostgresResearchStore::new(pool).control(&change(Target::Scope,3,false)).await});
        waiting(&h,pid).await?;barrier.rollback().await?;let receipt=completion.await??;disable.await??;
        sqlx::raw_sql("DROP TRIGGER control_barrier ON edgeagent_research_decisions").execute(&h.pool).await?;
        let p=prepared("scope","winner","winner",110,3)?;
        assert_eq!(store.recover(p.delivery()).await?.ok_or("winner receipt")?.bytes(),receipt.bytes());
        for trigger in ["CREATE TRIGGER control_fault BEFORE INSERT ON edgeagent_research_controls FOR EACH ROW EXECUTE FUNCTION control_fault()","CREATE CONSTRAINT TRIGGER control_fault AFTER INSERT ON edgeagent_research_controls DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION control_fault()"] {
            sqlx::raw_sql("CREATE OR REPLACE FUNCTION control_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-control-sentinel'; END $$").execute(&h.pool).await?;
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            let error=store.control(&change(Target::Scope,4,true)).await.err().ok_or("audit rollback")?;
            assert_eq!(error.kind(),Kind::Storage);assert!(!format!("{error:?} {error}").contains("private-control-sentinel"));
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT max(revision) FROM edgeagent_research_controls").fetch_one(&h.pool).await?,3);
            assert_eq!(store.commit(&prepared("scope","still-disabled","retry",110,3)?).await.err().ok_or("failed enable escaped")?.kind(),Kind::Disabled);
            sqlx::raw_sql("DROP TRIGGER control_fault ON edgeagent_research_controls").execute(&h.pool).await?;
        }
        store.control(&change(Target::Scope,4,true)).await?;
        let p=prepared("scope","unfenced","old-binary",110,3)?;
        let event=p.decision_publication().ok_or("decision")?;
        let decision=edgeagent_research_application::publication::decode_decision(event)?.canonical_bytes()?;
        let mut tx=h.pool.begin().await?;
        sqlx::query("INSERT INTO edgeagent_research_completions(caller_scope,request_id,request,outcome,publication) VALUES($1,$2,$3,$4,$5)").bind(p.request().scope.as_str()).bind(p.request().request_id.as_str()).bind(p.request_bytes()).bind(p.outcome_bytes()).bind(p.publication().to_json()?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO edgeagent_research_decisions(caller_scope,request_id,configuration,decision,publication) VALUES($1,$2,$3,$4,$5)").bind(p.request().scope.as_str()).bind(p.request().request_id.as_str()).bind(p.execution_bytes().ok_or("config")?).bind(decision).bind(event.to_json()?).execute(&mut *tx).await?;
        edgeagent_outbox_postgres::PostgresOutbox.enqueue(&mut tx,p.definition(),p.publication()).await?;
        assert!(tx.commit().await.is_err());assert_eq!(h.counts().await?,(1,1,1));
        // Simulate a pre-0005 validated decision: retain its companion but invent no control snapshot.
        let p=prepared("scope","pre-controls","historical",110,3)?;
        let event=p.decision_publication().ok_or("decision")?;
        let decision=edgeagent_research_application::publication::decode_decision(event)?.canonical_bytes()?;
        let mut tx=h.pool.begin().await?;
        sqlx::raw_sql("ALTER TABLE edgeagent_research_completions DISABLE TRIGGER edgeagent_research_require_decision; ALTER TABLE edgeagent_research_completions DISABLE TRIGGER edgeagent_research_require_controls").execute(&mut *tx).await?;
        sqlx::query("INSERT INTO edgeagent_research_completions(caller_scope,request_id,request,outcome,publication,requires_controls) VALUES($1,$2,$3,$4,$5,FALSE)").bind(p.request().scope.as_str()).bind(p.request().request_id.as_str()).bind(p.request_bytes()).bind(p.outcome_bytes()).bind(p.publication().to_json()?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO edgeagent_research_decisions(caller_scope,request_id,configuration,decision,publication) VALUES($1,$2,$3,$4,$5)").bind(p.request().scope.as_str()).bind(p.request().request_id.as_str()).bind(p.execution_bytes().ok_or("config")?).bind(decision).bind(event.to_json()?).execute(&mut *tx).await?;
        edgeagent_outbox_postgres::PostgresOutbox.enqueue(&mut tx,p.definition(),p.publication()).await?;
        edgeagent_outbox_postgres::PostgresOutbox.enqueue(&mut tx,edgeagent_research_application::publication::DECISION,event).await?;
        sqlx::raw_sql("ALTER TABLE edgeagent_research_completions ENABLE TRIGGER edgeagent_research_require_decision; ALTER TABLE edgeagent_research_completions ENABLE TRIGGER edgeagent_research_require_controls").execute(&mut *tx).await?;
        tx.commit().await?;store.control(&change(Target::Scope,5,false)).await?;
        assert_eq!(store.recover(p.delivery()).await?.ok_or("historical receipt")?.bytes(),p.outcome_bytes());
        let legacy:Option<i64>=sqlx::query_scalar("SELECT scope_control_revision FROM edgeagent_research_decisions WHERE request_id='pre-controls'").fetch_one(&h.pool).await?;
        assert!(legacy.is_none());

        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
