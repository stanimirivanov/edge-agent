//! Live private-schema checkpoint/inbox/progress, process failure and read-model proofs.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned test schema inspection and fault injection."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use edgeagent_audit_projector::error::Kind;
use edgeagent_contracts::research::ResearchRecord;
use futures_util::FutureExt;
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use support::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn pending_duplicate_restart_and_exact_queries_preserve_one_checkpoint() -> Result {
    let db = Database::new().await?;
    let result = AssertUnwindSafe(async {
        let store = Arc::new(db.store()?);
        let record = record("request-1", false)?;
        let request = request(&record)?;
        let event = event(&record)?;
        assert!(store.get("offline", "unknown").await?.is_none());
        store.track(request).await?;
        let pending = store.get("offline", "request-1").await?.ok_or("pending")?;
        assert_eq!(
            (pending.status, pending.projection_revision),
            ("pending", 0)
        );
        assert!(pending.outcome_bytes().is_none());
        let server = Server::new(store.clone()).await?;
        let response = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1/outcome",
                "",
            )
            .await?;
        assert!(response.starts_with("HTTP/1.1 202"));
        store.project(&event, "delivery-1").await?;
        let before: Vec<u8> = sqlx::query_scalar("SELECT checkpoint FROM edgeagent_research_views")
            .fetch_one(&db.pool)
            .await?;
        store.project(&event, "reordered-duplicate").await?;
        let mut child = db.spawn(&event.to_json()?).await?;
        assert!(
            tokio::time::timeout(Duration::from_secs(15), child.wait())
                .await??
                .success()
        );
        let view = store.get("offline", "request-1").await?.ok_or("view")?;
        assert_eq!((view.status, view.projection_revision), ("artifact", 1));
        assert_eq!(view.applied_delivery_key.as_deref(), Some("delivery-1"));
        assert_eq!(
            view.outcome_bytes(),
            Some(record.canonical_bytes()?.as_slice())
        );
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>("SELECT checkpoint FROM edgeagent_research_views")
                .fetch_one(&db.pool)
                .await?,
            before
        );
        assert_eq!(db.counts().await?, (1, 1));
        let response = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1/outcome",
                "",
            )
            .await?;
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(body(&response)?.as_bytes(), record.canonical_bytes()?);
        let meta = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1",
                "",
            )
            .await?;
        assert!(body(&meta)?.contains("\"status\":\"artifact\""));
        let unknown = server
            .query("GET", "/internal/v1/research-requests/offline/unknown", "")
            .await?;
        assert!(unknown.starts_with("HTTP/1.1 404"));
        assert!(body(&unknown)?.contains("not_observed"));
        let abstained = support::record("abstained", true)?;
        store
            .project(&support::event(&abstained)?, "delivery-2")
            .await?;
        assert_eq!(
            store
                .get("offline", "abstained")
                .await?
                .ok_or("abstained")?
                .status,
            "abstained"
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn statement_commit_and_worker_death_rollback_checkpoint_inbox_and_progress() -> Result {
    let db = Database::new().await?;
    let result=AssertUnwindSafe(async {
        let store=db.store()?;let event=event(&record("request-1",false)?)?;
        sqlx::raw_sql("CREATE FUNCTION test_refuse_projection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-test-sentinel'; END; $$").execute(&db.pool).await?;
        for (table,trigger) in [
            ("edgeagent_message_inbox","CREATE TRIGGER test_refuse BEFORE INSERT ON edgeagent_message_inbox FOR EACH ROW EXECUTE FUNCTION test_refuse_projection()"),
            ("edgeagent_research_views","CREATE CONSTRAINT TRIGGER test_refuse AFTER UPDATE ON edgeagent_research_views DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION test_refuse_projection()"),
        ] {
            sqlx::raw_sql(trigger).execute(&db.pool).await?;
            let error=store.project(&event,"delivery-1").await.err().ok_or("expected rollback")?;assert_eq!(error.kind(),Kind::Storage);assert!(!format!("{error:?}").contains("private-test-sentinel"));assert_eq!(db.counts().await?,(0,0));
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TRIGGER test_refuse ON {table}"))).execute(&db.pool).await?;
        }
        let mut barrier=db.pool.acquire().await?;sqlx::query("SELECT pg_advisory_lock($1)").bind(db.key).execute(&mut *barrier).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION test_projection_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({}); RETURN NEW; END; $$; CREATE TRIGGER test_barrier BEFORE UPDATE ON edgeagent_research_views FOR EACH ROW EXECUTE FUNCTION test_projection_barrier()",db.key))).execute(&db.pool).await?;
        let mut child=db.spawn(&event.to_json()?).await?;
        let waiting=tokio::time::timeout(Duration::from_secs(5),async {
            loop {let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND NOT granted AND classid::bigint=$1 AND objid::bigint=$2)").bind(db.key>>32).bind(db.key & i64::from(u32::MAX)).fetch_one(&db.pool).await?;if blocked {return Ok::<_,sqlx::Error>(());}tokio::task::yield_now().await;}
        }).await;
        child.kill().await?;sqlx::query("SELECT pg_advisory_unlock($1)").bind(db.key).execute(&mut *barrier).await?;
        sqlx::query("SELECT pg_advisory_lock($1)").bind(db.key).execute(&mut *barrier).await?;sqlx::query("SELECT pg_advisory_unlock($1)").bind(db.key).execute(&mut *barrier).await?;waiting??;drop(barrier);
        assert_eq!(db.counts().await?,(0,0));sqlx::raw_sql("DROP TRIGGER test_barrier ON edgeagent_research_views").execute(&db.pool).await?;
        store.project(&event,"after-restart").await?;assert_eq!(db.counts().await?,(1,1));Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn conflicting_owner_data_stays_gapped_and_corrupt_or_unavailable_views_fail_closed() -> Result
{
    let db = Database::new().await?;
    let result = AssertUnwindSafe(async {
        let store = Arc::new(db.store()?);
        let record = record("request-1", false)?;
        store.project(&event(&record)?, "first").await?;
        let checkpoint: Vec<u8> =
            sqlx::query_scalar("SELECT checkpoint FROM edgeagent_research_views")
                .fetch_one(&db.pool)
                .await?;
        let mut changed = record;
        if let ResearchRecord::Artifact(a) = &mut changed {
            a.expires_at_ms += 1;
        }
        assert_eq!(
            store
                .project(&event(&changed)?, "conflict")
                .await
                .err()
                .ok_or("gap")?
                .kind(),
            Kind::Gap
        );
        let view = store.get("offline", "request-1").await?.ok_or("view")?;
        assert_eq!(view.status, "gap");
        assert_eq!(view.applied_delivery_key.as_deref(), Some("first"));
        assert!(view.outcome_bytes().is_none());
        assert_eq!(db.counts().await?, (1, 1));
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>("SELECT checkpoint FROM edgeagent_research_views")
                .fetch_one(&db.pool)
                .await?,
            checkpoint
        );
        let server = Server::new(store.clone()).await?;
        let response = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1/outcome",
                "",
            )
            .await?;
        assert!(response.starts_with("HTTP/1.1 409"));
        let response = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1",
                "",
            )
            .await?;
        assert!(body(&response)?.contains("\"status\":\"gap\""));
        let identity = support::record("identity", false)?;
        let envelope = event(&identity)?;
        store.project(&envelope, "identity-first").await?;
        let mut reused: serde_json::Value = serde_json::from_slice(&envelope.to_json()?)?;
        reused["time"] = "2026-10-29T00:00:01Z".into();
        let reused =
            edgeagent_contracts::MessageEnvelope::from_json(&serde_json::to_vec(&reused)?)?;
        assert_eq!(
            store
                .project(&reused, "identity-reuse")
                .await
                .err()
                .ok_or("identity gap")?
                .kind(),
            Kind::Gap
        );
        assert_eq!(
            store
                .get("offline", "identity")
                .await?
                .ok_or("identity")?
                .status,
            "gap"
        );
        assert_eq!(db.counts().await?, (2, 2));
        sqlx::query("UPDATE edgeagent_research_views SET checkpoint=$1")
            .bind([0u8; 32].as_slice())
            .execute(&db.pool)
            .await?;
        let response = server
            .query(
                "GET",
                "/internal/v1/research-requests/offline/request-1",
                "",
            )
            .await?;
        assert!(response.starts_with("HTTP/1.1 503"));
        db.pool.close().await;
        let response = server
            .query("GET", "/internal/v1/research-requests/offline/unknown", "")
            .await?;
        assert!(response.starts_with("HTTP/1.1 503"));
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn lifecycle_gaps_backfill_atomically_and_closed_queries_preserve_exact_history() -> Result {
    use edgeagent_contracts::research::{LifecycleChange, ResearchLifecycle};
    use edgeagent_research_application::lifecycle;
    let db = Database::new().await?;
    let result=AssertUnwindSafe(async {
        let store=Arc::new(db.store()?);let record=record("life",false)?;
        let req=request(&record)?;
        let first=ResearchLifecycle{scope:req.scope.clone(),request_id:req.request_id.clone(),artifact:record.digest()?,revision:1,effective_at_ms:100,change:LifecycleChange::Invalidated};
        let mut last=first.clone();last.revision=2;last.change=LifecycleChange::Withdrawn;
        let one=lifecycle::publication(&first,"2026-10-29T00:00:00Z")?;
        let two=lifecycle::publication(&last,"2026-10-29T00:00:00Z")?;
        assert_eq!(store.project(&two,"missing-base").await.err().ok_or("gap")?.kind(),Kind::Gap);
        assert_eq!(db.counts().await?,(0,0));
        store.track(req).await?;
        assert_eq!(store.project(&two,"gap").await.err().ok_or("gap")?.kind(),Kind::Gap);
        store.project(&event(&record)?,"base").await?;
        store.project(&one,"one").await?;
        assert_eq!(store.get("offline","life").await?.ok_or("view")?.status,"gap");
        sqlx::raw_sql("CREATE FUNCTION clear_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.gap_envelope IS NULL THEN RAISE EXCEPTION 'synthetic clear fault'; END IF; RETURN NEW; END $$; CREATE TRIGGER clear_fault BEFORE UPDATE OF gap_envelope ON edgeagent_research_views FOR EACH ROW EXECUTE FUNCTION clear_fault()").execute(&db.pool).await?;
        assert_eq!(store.project(&two,"two").await.err().ok_or("fault")?.kind(),Kind::Storage);
        assert_eq!(db.counts().await?,(1,2));
        assert_eq!(store.get("offline","life").await?.ok_or("view")?.projection_revision,2);
        sqlx::raw_sql("DROP TRIGGER clear_fault ON edgeagent_research_views").execute(&db.pool).await?;
        store.project(&two,"two").await?;
        let mut child=db.spawn(&two.to_json()?).await?;
        assert!(tokio::time::timeout(Duration::from_secs(15),child.wait()).await??.success());
        let view=store.get("offline","life").await?.ok_or("view")?;
        assert_eq!((view.status,view.projection_revision),("withdrawn",3));
        assert_eq!(view.applied_delivery_key.as_deref(),Some("two"));
        assert_eq!(view.outcome_bytes(),Some(record.canonical_bytes()?.as_slice()));
        assert!(view.lifecycle==Some(last));assert_eq!(db.counts().await?,(1,3));
        let server=Server::new(store.clone()).await?;
        assert!(server.query("GET","/internal/v1/research-requests/offline/life/outcome","").await?.starts_with("HTTP/1.1 410"));
        let metadata=server.query("GET","/internal/v1/research-requests/offline/life","").await?;
        assert!(metadata.starts_with("HTTP/1.1 200"));assert!(metadata.contains("withdrawn"));
        sqlx::query("DELETE FROM edgeagent_message_inbox WHERE message_id=$1").bind(one.id()).execute(&db.pool).await?;
        assert_eq!(store.get("offline","life").await.err().ok_or("proof")?.kind(),Kind::Corrupt);
        Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}
