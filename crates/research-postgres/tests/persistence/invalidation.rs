use super::*;
use edgeagent_contracts::{
    research::{LifecycleChange, ResearchRecord},
    research_invalidation::Command,
};
fn check(
    prepared: &edgeagent_research_application::publication::PreparedCompletion,
    at: i64,
    next: Option<&edgeagent_research_application::publication::PreparedCompletion>,
) -> Command {
    Command::V1 {
        original: prepared.request().clone(),
        effective_at_ms: at,
        reassessment: next.map(|p| p.request().clone()),
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn retained_reassessments_close_only_opted_in_artifacts_and_preserve_originals() -> Result {
    let h = Harness::approved().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let profile = execution(14)?.profile("scope");
        store.govern(&profile,1).await?; store.govern(&profile,2).await?;
        let base = configured("original",100,110,true)?;
        let same = configured("same",102,110,true)?;
        let denied = configured("denied",105,90,true)?;
        let incomplete = configured("incomplete",104,110,false)?;
        assert!(matches!(ResearchRecord::decode(base.outcome_bytes())?, ResearchRecord::Artifact(_)), "{}", String::from_utf8_lossy(base.outcome_bytes()));
        let original = store.commit(&base).await?;
        for p in [&same,&denied,&incomplete] { store.commit(p).await?; }
        let legacy=prepared("scope","legacy","legacy",110,3)?;
        store.commit(&legacy).await?;
        let before = h.counts().await?;
        assert!(store.check_invalidation("scope", &check(&base,102,Some(&same)),TIME).await?.is_none());
        assert!(store.check_invalidation("scope", &check(&base,104,Some(&incomplete)),TIME).await.is_err());
        assert!(store.check_invalidation("scope", &check(&base,104,None),TIME).await.is_err());
        let missing = configured("missing",103,90,true)?;
        assert!(store.check_invalidation("scope", &check(&base,104,Some(&missing)),TIME).await.is_err());
        assert!(store.check_invalidation("foreign", &check(&base,105,Some(&denied)),TIME).await.is_err());
        assert_eq!(h.counts().await?,before);
        // Kill switches/retirement do not retroactively prevent lifecycle maintenance.
        store.govern(&profile,3).await?;
        store.control(&edgeagent_contracts::research_controls::Command::V1 {scope:"scope".into(),target:edgeagent_contracts::research_controls::Target::Scope,revision:1,enabled:false,reason_code:"stop".into()}).await?;
        let command = check(&base,105,Some(&denied));
        let (a,b) = tokio::join!(store.check_invalidation("scope",&command,TIME),store.check_invalidation("scope",&command,TIME));
        let a=a?.ok_or("event")?; assert_eq!(a.to_json()?,b?.ok_or("event")?.to_json()?);
        assert!(matches!(edgeagent_research_application::lifecycle::decode(&a)?.change,LifecycleChange::Invalidated));
        assert_eq!(a.extension("causationid"),denied.decision_publication().map(|e|e.id()));
        assert_eq!(h.counts().await?,(before.0,before.1,before.2+1));
        let restarted = PostgresResearchStore::new(h.reconnect().await?);
        assert_eq!(restarted.check_invalidation("scope",&command,TIME).await?.ok_or("retry")?.to_json()?,a.to_json()?);
        assert_eq!(store.recover_existing(base.delivery()).await?.ok_or("original")?.bytes(),original.bytes());
        // The strict legacy configuration remains explicit-only even after its cutoff.
        let legacy_command=check(&legacy,110,None);
        assert!(store.check_invalidation("scope",&legacy_command,TIME).await.is_err());
        // Remove only test fixture audit corruption; retained decision validation must refuse.
        sqlx::raw_sql("ALTER TABLE edgeagent_research_decisions DISABLE TRIGGER edgeagent_research_decisions_immutable; UPDATE edgeagent_research_decisions SET decision='bad'::bytea WHERE request_id='denied'; ALTER TABLE edgeagent_research_decisions ENABLE TRIGGER edgeagent_research_decisions_immutable").execute(&h.pool).await?;
        assert!(store.check_invalidation("scope",&check(&same,105,Some(&denied)),TIME).await.is_err());
        assert!(store.retained_for_replay(denied.delivery()).await.is_err());
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn configured_expiry_and_invalidation_rollback_atomically_and_serialize_with_owner_commands()
-> Result {
    let h = Harness::approved().await?;
    let result = AssertUnwindSafe(async {
        let store = PostgresResearchStore::new(h.pool.clone());
        let profile=execution(14)?.profile("scope"); store.govern(&profile,1).await?;store.govern(&profile,2).await?;
        let base=configured("expiry",100,110,true)?;store.commit(&base).await?;
        let command=check(&base,110,None);
        let before=h.counts().await?;
        sqlx::raw_sql("CREATE FUNCTION invalidation_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-invalidation-sentinel'; END $$").execute(&h.pool).await?;
        for trigger in ["CREATE TRIGGER invalidation_fault BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION invalidation_fault()", "CREATE CONSTRAINT TRIGGER invalidation_fault AFTER INSERT ON edgeagent_research_lifecycle DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION invalidation_fault()"] {
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            let error=store.check_invalidation("scope",&command,TIME).await.err().ok_or("fault")?;
            assert_eq!(error.kind(),Kind::Storage);assert!(!format!("{error:?} {error}").contains("private-invalidation-sentinel"));
            assert_eq!(h.counts().await?,before);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_lifecycle").fetch_one(&h.pool).await?,0);
            sqlx::raw_sql(if trigger.starts_with("CREATE CONSTRAINT") {"DROP TRIGGER invalidation_fault ON edgeagent_research_lifecycle"} else {"DROP TRIGGER invalidation_fault ON edgeagent_message_outbox"}).execute(&h.pool).await?;
        }
        let ResearchRecord::Artifact(artifact)=ResearchRecord::decode(base.outcome_bytes())? else {return Err("artifact".into());};
        let event=edgeagent_contracts::research::ResearchLifecycle {scope:"scope".into(),request_id:artifact.request.request_id.clone(),artifact:ResearchRecord::Artifact(artifact).digest()?,revision:1,effective_at_ms:110,change:LifecycleChange::Expired};
        let (a,b)=tokio::join!(store.check_invalidation("scope",&command,TIME),store.append_lifecycle("scope",event,TIME));
        assert_eq!(a?.ok_or("expiry")?.to_json()?,b?.to_json()?);
        assert_eq!(h.counts().await?,(1,1,2));
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
