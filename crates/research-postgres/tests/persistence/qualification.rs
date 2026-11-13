use super::*;
#[path = "../../../research-application/tests/support/qualification.rs"]
mod fixture;
use edgeagent_contracts::explanation_qualification::Command;
use edgeagent_research_application::explanation_qualification::VerifiedReview;
use edgeagent_research_postgres::explanation_qualification::MIGRATION;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn qualification_approvals_are_exact_audited_revocable_and_fenced() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        sqlx::raw_sql(MIGRATION).execute(&h.pool).await?;
        let command=fixture::approved()?;let profile=command.profile().clone();let store=PostgresResearchStore::new(h.pool.clone());
        assert!(store.qualification_approval("scope",&profile).await?.is_none());
        let verified=VerifiedReview::decode(&command.bytes()?)?;
        store.review_qualification(&verified).await?;store.review_qualification(&verified).await?;
        let observed=store.qualification_approval("scope",&profile).await?.ok_or("approval")?;
        assert_eq!(observed.revision,1);assert_eq!(observed.manifest,"e".repeat(64));assert_eq!(observed.helper,"f".repeat(64));assert_eq!(observed.suite,edgeagent_research_application::explanation_evaluation::Suite::complete()?.identity());assert_eq!(observed.assessment,verified.assessment());assert_eq!(observed.collection,verified.collection());
        let mut foreign=profile.clone();foreign.settings.seed=2;
        assert!(store.qualification_approval("scope",&foreign).await?.is_none());assert!(store.qualification_approval("other",&profile).await?.is_none());
        let mut conflict=command.clone();let Command::Approve{attestation,..}=&mut conflict else {return Err("approve".into());};*attestation="a".repeat(64);
        assert!(store.review_qualification(&VerifiedReview::decode(&conflict.bytes()?)?).await.is_err());
        let revoked=Command::Revoke{scope:"scope".into(),revision:2,attestation:"a".repeat(64),profile:profile.clone()};let revoke=VerifiedReview::decode(&revoked.bytes()?)?;
        let mut tx=h.pool.begin().await?;
        PostgresResearchStore::current_qualification_approval(&mut tx,"scope",&profile).await?.ok_or("current")?;
        let mut rival=h.pool.begin().await?;let key=format!("explanation-qualification/scope/{}",profile.id()?);
        let available:bool=sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))").bind(&key).fetch_one(&mut *rival).await?;assert!(!available);rival.rollback().await?;tx.commit().await?;
        store.review_qualification(&revoke).await?;assert!(store.qualification_approval("scope",&profile).await?.is_none());assert!(store.review_qualification(&verified).await.is_err());
        let mut next=command.clone();let Command::Approve{revision,..}=&mut next else {return Err("approve".into());};*revision=3;let next=VerifiedReview::decode(&next.bytes()?)?;
        sqlx::raw_sql("CREATE FUNCTION reject_qualification() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'qualification-test'; END $$; CREATE CONSTRAINT TRIGGER reject_qualification AFTER INSERT ON edgeagent_explanation_qualification DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_qualification()").execute(&h.pool).await?;
        assert!(store.review_qualification(&next).await.is_err());assert!(store.qualification_approval("scope",&profile).await?.is_none());
        sqlx::raw_sql("DROP TRIGGER reject_qualification ON edgeagent_explanation_qualification").execute(&h.pool).await?;
        let (a,b)=tokio::join!(store.review_qualification(&next),store.review_qualification(&next));a?;b?;
        let pool=h.reconnect().await?;assert_eq!(PostgresResearchStore::new(pool.clone()).qualification_approval("scope",&profile).await?.ok_or("restart")?.revision,3);pool.close().await;
        assert!(sqlx::query("UPDATE edgeagent_explanation_qualification SET approved=FALSE").execute(&h.pool).await.is_err());assert!(sqlx::query("DELETE FROM edgeagent_explanation_qualification").execute(&h.pool).await.is_err());assert!(sqlx::query("TRUNCATE edgeagent_explanation_qualification").execute(&h.pool).await.is_err());
        let audit:bool=sqlx::query_scalar("SELECT count(*)=3 AND bool_and(actor=session_user AND recorded_at IS NOT NULL) FROM edgeagent_explanation_qualification").fetch_one(&h.pool).await?;assert!(audit);
        // Authenticate a real independent LOGIN role, not SET ROLE on an owner session.
        let role=format!("qualification_guest_{}",h.key);let schema:String=sqlx::query_scalar("SELECT current_schema()").fetch_one(&h.pool).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE ROLE {role} LOGIN PASSWORD '{role}' NOSUPERUSER NOINHERIT; GRANT USAGE ON SCHEMA {schema} TO {role}; GRANT SELECT,INSERT ON edgeagent_explanation_qualification TO {role}"))).execute(&h.pool).await?;
        let options=std::env::var("EDGEAGENT_POSTGRES_URL")?.parse::<sqlx::postgres::PgConnectOptions>()?.username(&role).password(&role).options([("search_path",schema.as_str())]);
        let guest=sqlx::postgres::PgPoolOptions::new().max_connections(1).connect_with(options).await?;
        let denied=AssertUnwindSafe(async {
            assert!(PostgresResearchStore::new(guest.clone()).review_qualification(&next).await.is_err());
            let error=sqlx::query("INSERT INTO edgeagent_explanation_qualification SELECT caller_scope,profile,4,command,assessment,collection,approved,actor,recorded_at FROM edgeagent_explanation_qualification WHERE revision=3").execute(&guest).await.err().ok_or("nonowner insert")?;
            assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }).catch_unwind().await;
        guest.close().await;sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role}; DROP ROLE {role}"))).execute(&h.pool).await?;
        match denied {Ok(r)=>r?,Err(panic)=>std::panic::resume_unwind(panic)};
        // Retained assessment/command corruption must never become approval.
        sqlx::raw_sql("ALTER TABLE edgeagent_explanation_qualification DISABLE TRIGGER USER").execute(&h.pool).await?;
        sqlx::query("UPDATE edgeagent_explanation_qualification SET assessment=repeat('a',64) WHERE revision=3").execute(&h.pool).await?;
        assert!(store.qualification_approval("scope",&profile).await.is_err());Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
