//! Real transaction/HTTP proofs in private disposable PostgreSQL schemas.
#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema inspection and fault injection."
)]
#[path = "support/database.rs"]
mod database;
mod support;
use database::Harness;
use edgeagent_gateway::admission::{AdmissionStore, Kind, PostgresAdmission};
use edgeagent_research_application::publication;
use futures_util::FutureExt;
use std::{panic::AssertUnwindSafe, sync::Arc};
use support::*;
const JSON: &str = "Content-Type: application/json\r\n";
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn http_retries_restart_and_conflicting_reuse_preserve_one_command() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = Arc::new(PostgresAdmission::new(h.pool.clone(), "operator".into())?);
        let server = Server::new(store).await?;
        let body = serde_json::to_vec(&request())?;
        let one = server.post(&body, JSON).await?;
        assert!(one.starts_with("HTTP/1.1 202"));
        let original: Vec<u8> =
            sqlx::query_scalar("SELECT command FROM edgeagent_gateway_requests")
                .fetch_one(&h.pool)
                .await?;
        let envelope = edgeagent_contracts::MessageEnvelope::from_json(&original)?;
        assert!(publication::evaluation_request(&envelope)? == request());
        assert!(
            server
                .post(&serde_json::to_vec_pretty(&request())?, JSON)
                .await?
                .starts_with("HTTP/1.1 202")
        );
        drop(server);
        let pool = h.reconnect().await?;
        let restarted = Server::new(Arc::new(PostgresAdmission::new(
            pool.clone(),
            "operator".into(),
        )?))
        .await?;
        // The client may have lost any earlier response; reuse the same identity.
        assert!(
            restarted
                .post(&body, JSON)
                .await?
                .starts_with("HTTP/1.1 202")
        );
        let mut changed = request();
        changed.decision_at_ms += 1;
        assert!(
            restarted
                .post(&serde_json::to_vec(&changed)?, JSON)
                .await?
                .starts_with("HTTP/1.1 409")
        );
        assert_eq!(h.counts().await?, (1, 1));
        let retained: Vec<u8> =
            sqlx::query_scalar("SELECT command FROM edgeagent_gateway_requests")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(original, retained);
        drop(restarted);
        pool.close().await;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn concurrent_admission_and_separate_scopes_are_idempotent() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresAdmission::new(h.pool.clone(), "operator".into())?;
        let (a, b) = tokio::join!(store.admit(request()), store.admit(request()));
        assert_eq!(a?, b?);
        assert_eq!(h.counts().await?, (1, 1));
        let mut foreign = request();
        foreign.scope = "another".into();
        assert_eq!(
            store
                .admit(foreign.clone())
                .await
                .err()
                .ok_or("expected rejection")?
                .kind(),
            Kind::Invalid
        );
        PostgresAdmission::new(h.pool.clone(), "another".into())?
            .admit(foreign)
            .await?;
        assert_eq!(h.counts().await?, (2, 2));
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn statement_and_deferred_commit_failures_never_acknowledge_or_retain_intent() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let server=Server::new(Arc::new(PostgresAdmission::new(h.pool.clone(),"operator".into())?)).await?;
        sqlx::raw_sql("CREATE FUNCTION test_refuse_gateway() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-test-sentinel'; END; $$").execute(&h.pool).await?;
        for trigger in ["CREATE TRIGGER test_refuse BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION test_refuse_gateway()","CREATE CONSTRAINT TRIGGER test_refuse AFTER INSERT ON edgeagent_message_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION test_refuse_gateway()"] {
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            let response=server.post(&serde_json::to_vec(&request())?,JSON).await?;
            assert!(response.starts_with("HTTP/1.1 503"));assert!(!response.contains("private-test-sentinel"));assert_eq!(h.counts().await?,(0,0));
            sqlx::raw_sql("DROP TRIGGER test_refuse ON edgeagent_message_outbox").execute(&h.pool).await?;
        }
        assert!(server.post(&serde_json::to_vec(&request())?,JSON).await?.starts_with("HTTP/1.1 202"));assert_eq!(h.counts().await?,(1,1));Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn immutable_admission_and_corrupt_retained_outbox_fail_closed() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresAdmission::new(h.pool.clone(), "operator".into())?;
        store.admit(request()).await?;
        for sql in [
            "UPDATE edgeagent_gateway_requests SET request=request",
            "DELETE FROM edgeagent_gateway_requests",
        ] {
            assert!(sqlx::query(sql).execute(&h.pool).await.is_err());
        }
        sqlx::query("UPDATE edgeagent_message_outbox SET transport_subject='changed'")
            .execute(&h.pool)
            .await?;
        assert_eq!(
            store
                .admit(request())
                .await
                .err()
                .ok_or("expected integrity refusal")?
                .kind(),
            Kind::Storage
        );
        sqlx::query("DELETE FROM edgeagent_message_outbox")
            .execute(&h.pool)
            .await?;
        assert_eq!(
            store
                .admit(request())
                .await
                .err()
                .ok_or("expected integrity refusal")?
                .kind(),
            Kind::Storage
        );
        assert_eq!(h.counts().await?, (1, 0));
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
