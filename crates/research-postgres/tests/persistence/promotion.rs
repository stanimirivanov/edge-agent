use super::*;

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn promotion_requires_sealed_matching_evidence_and_commits_atomically() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let profile = execution(3)?.profile("scope");
        let store = PostgresResearchStore::new(h.pool.clone());
        let (promotion, commands) = promotion_fixture::promotion(&profile)?;
        store.govern(&profile, 1).await?;
        assert!(store.govern(&profile, 2).await.is_err());
        assert!(legacy_insert(&h.pool).await.is_err());
        assert!(edgeagent_contracts::research_promotion::Promotion::decode(&promotion.bytes()?)? == promotion);

        for command in &commands[..2] {
            store.experiment_ledger(command).await?;
        }
        assert!(store.promote(&promotion).await.is_err());
        store.experiment_ledger(&commands[2]).await?;
        let mut wrong = promotion.clone();
        wrong.trial = "wrong".into();
        assert!(store.promote(&wrong).await.is_err());
        let mut overlap = promotion.clone();
        overlap.holdout_start_ms = 100;
        assert!(store.promote(&overlap).await.is_err());
        sqlx::raw_sql("CREATE FUNCTION test_refuse_promotion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test promotion refusal'; END $$; CREATE CONSTRAINT TRIGGER refuse_promotion AFTER INSERT ON edgeagent_research_approvals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (NEW.revision=2) EXECUTE FUNCTION test_refuse_promotion()").execute(&h.pool).await?;
        assert!(store.promote(&promotion).await.is_err());
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_approvals WHERE revision=2").fetch_one(&h.pool).await?,0);
        sqlx::raw_sql("DROP TRIGGER refuse_promotion ON edgeagent_research_approvals").execute(&h.pool).await?;
        let (a,b)=tokio::join!(store.promote(&promotion),store.promote(&promotion));
        a?; b?;
        let saved: Vec<u8> = sqlx::query_scalar("SELECT promotion FROM edgeagent_research_approvals WHERE revision=2").fetch_one(&h.pool).await?;
        assert_eq!(saved, promotion.bytes()?);
        let mut changed = promotion.clone();
        changed.report = "cd".repeat(32);
        assert!(store.promote(&changed).await.is_err());
        store.govern(&profile, 3).await?;
        assert!(store.promote(&promotion).await.is_err());
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}

async fn legacy_insert(pool: &sqlx::PgPool) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO edgeagent_research_approvals(caller_scope,implementation,configuration,revision,action,actor) SELECT caller_scope,implementation,configuration,2,'approved',session_user FROM edgeagent_research_profiles")
        .execute(pool).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn promotion_migration_preserves_existing_approvals_and_recovery() -> Result {
    let h = Harness::with_promotion_migration(false).await?;
    let result = AssertUnwindSafe(async {
        let profile = execution(3)?.profile("scope");
        sqlx::query("INSERT INTO edgeagent_research_profiles VALUES ($1,$2,$3,$4)")
            .bind(&profile.scope)
            .bind(&profile.implementation)
            .bind(&profile.configuration)
            .bind(profile.bytes()?)
            .execute(&h.pool)
            .await?;
        legacy_insert(&h.pool).await?;
        sqlx::raw_sql(edgeagent_research_postgres::governance::PROMOTION_MIGRATION)
            .execute(&h.pool)
            .await?;
        let store = PostgresResearchStore::new(h.pool.clone());
        store.govern(&profile, 2).await?;
        let p = prepared("scope", "legacy-promotion", "legacy-promotion", 110, 3)?;
        let receipt = store.commit(&p).await?;
        store.govern(&profile, 3).await?;
        assert_eq!(
            store
                .recover_existing(p.delivery())
                .await?
                .ok_or("receipt")?
                .bytes(),
            receipt.bytes()
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
