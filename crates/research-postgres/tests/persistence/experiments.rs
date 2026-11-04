use super::*;
use edgeagent_contracts::{
    research::ResearchRecord,
    research_experiment::{Action, Command, Failure, Terminal, Trial},
};

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn experiment_ledger_fences_concurrent_retries_and_recovers_an_immutable_census() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let ResearchRecord::Artifact(artifact) = ResearchRecord::decode(include_bytes!(
            "../../../research-application/tests/fixtures/artifact-v1.json"
        ))? else { return Err("expected artifact".into()); };
        let trial = artifact.request.request_id.clone();
        let command = |action| Command::V1 { scope: artifact.request.scope.clone(), family: "study".into(), action };
        let register = command(Action::Register {
            protocol: "ab".repeat(32),
            trials: vec![Trial { request: artifact.request.clone(), evidence: artifact.evidence }],
        });
        let finish = command(Action::Finish { trial: trial.clone(), outcome: Terminal::Failed { reason: Failure::Unavailable } });
        let read = command(Action::Read);
        let store = PostgresResearchStore::new(h.pool.clone());
        assert!(store.experiment_ledger(&finish).await.is_err());
        let (first, second) = tokio::join!(store.experiment_ledger(&register), store.experiment_ledger(&register));
        assert_eq!(first?, second?);
        sqlx::raw_sql("CREATE FUNCTION test_refuse_trial() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test trial refusal'; END $$; CREATE CONSTRAINT TRIGGER refuse_trial AFTER INSERT ON edgeagent_research_experiment_ledger DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (NEW.ordinal=1) EXECUTE FUNCTION test_refuse_trial()").execute(&h.pool).await?;
        assert!(store.experiment_ledger(&finish).await.is_err());
        assert_eq!(store.experiment_ledger(&read).await?, register.bytes()?);
        sqlx::raw_sql("DROP TRIGGER refuse_trial ON edgeagent_research_experiment_ledger").execute(&h.pool).await?;
        store.experiment_ledger(&finish).await?;
        assert!(store.experiment_ledger(&command(Action::Select { trial })).await.is_err());
        let restarted = PostgresResearchStore::new(h.reconnect().await?);
        assert_eq!(restarted.experiment_ledger(&read).await?, [register.bytes()?,finish.bytes()?].concat());
        assert_eq!(h.counts().await?, (0,0,0));
        assert!(sqlx::query("DELETE FROM edgeagent_research_experiment_ledger").execute(&h.pool).await.is_err());
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
