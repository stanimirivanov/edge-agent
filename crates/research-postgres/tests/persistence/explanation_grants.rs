use super::*;
use edgeagent_contracts::explanation::Anchor;
use edgeagent_research_application::explanation_context::{Grant, Purpose, Request};

pub(super) fn grant(purpose: Purpose) -> Grant {
    Grant {
        request: Request {
            anchor: Anchor {
                scope: "scope".into(),
                artifact: "a".repeat(64),
                decision: "b".repeat(64),
            },
            audience: "local-review".into(),
            purpose,
            document: None,
        },
        valid_from_ms: 100,
        expires_at_ms: 200,
        revoked: false,
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn explanation_grants_are_current_purpose_bound_and_transaction_fenced() -> Result {
    let h = Harness::approved().await?;
    let result = AssertUnwindSafe(async {
        sqlx::raw_sql(edgeagent_research_postgres::explanation_grants::MIGRATION).execute(&h.pool).await?;
        let store = PostgresResearchStore::new(h.pool.clone());
        let model = grant(Purpose::ModelDisclosure);
        let serving = grant(Purpose::ExplanationServing);
        let mut tx = h.pool.begin().await?;
        assert!(PostgresResearchStore::current_explanation_grant(&mut tx, &model.request, 100).await?.is_none());
        tx.commit().await?;
        store.set_explanation_grant(&model, 1).await?;
        store.set_explanation_grant(&model, 1).await?;
        store.set_explanation_grant(&serving, 1).await?;
        let revoked = Grant { revoked: true, ..model.clone() };
        assert_eq!(store.set_explanation_grant(&revoked, 1).await.err().ok_or("revision conflict")?.kind(), Kind::Conflict);
        assert_eq!(store.set_explanation_grant(&revoked, 3).await.err().ok_or("revision gap")?.kind(), Kind::Conflict);

        let mut tx = h.pool.begin().await?;
        let read = PostgresResearchStore::current_explanation_grant;
        assert_eq!(read(&mut tx, &model.request, 100).await?.ok_or("model grant")?.revision, 1);
        assert!(read(&mut tx, &model.request, 99).await?.is_none());
        assert!(read(&mut tx, &model.request, 200).await?.is_none());
        let foreign = Request { audience: "foreign".into(), ..model.request.clone() };
        assert!(read(&mut tx, &foreign, 100).await?.is_none());
        let document = Request { document: Some("d".repeat(64)), ..model.request.clone() };
        assert!(read(&mut tx, &document, 100).await?.is_none());
        let mut other = h.pool.begin().await?;
        let can_revoke: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended('scope',0))")
            .fetch_one(&mut *other).await?;
        assert!(!can_revoke);
        other.rollback().await?;
        tx.commit().await?;

        store.set_explanation_grant(&revoked, 2).await?;
        assert_eq!(store.set_explanation_grant(&model, 1).await.err().ok_or("stale revision")?.kind(), Kind::Conflict);
        let mut tx = h.pool.begin().await?;
        assert!(read(&mut tx, &model.request, 100).await?.is_none());
        assert_eq!(read(&mut tx, &serving.request, 100).await?.ok_or("serving grant")?.revision, 1);
        tx.commit().await?;
        let audited: bool = sqlx::query_scalar("SELECT count(*)=3 AND bool_and(actor=session_user AND occurred_at IS NOT NULL) FROM edgeagent_explanation_grants").fetch_one(&h.pool).await?;
        assert!(audited);
        assert!(sqlx::query("DELETE FROM edgeagent_explanation_grants").execute(&h.pool).await.is_err());
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
