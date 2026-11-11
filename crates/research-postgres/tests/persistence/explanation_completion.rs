use super::*;
use edgeagent_contracts::explanation::{Anchor, Binding, Companion, Fallback, Job, Refusal};
use edgeagent_research_application::{
    explanation::Allowlist,
    explanation_context::{Grant, Purpose},
    publication::decode_decision,
};
use edgeagent_research_postgres::{
    explanation_attempts::{Intent, Recovery, ReservationOutcome},
    explanation_completion::Completion,
};

async fn counts(h: &Harness) -> Result<(i64, i64)> {
    Ok(sqlx::query_as("SELECT (SELECT count(*) FROM edgeagent_explanation_completions), (SELECT count(*) FROM edgeagent_message_outbox WHERE message_type='com.edgeagent.research.explanation-recorded.v1')")
        .fetch_one(&h.pool).await?)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn explanation_completion_rolls_back_and_recovers_exact_first_terminal() -> Result {
    let h = Harness::approved().await?;
    let result = AssertUnwindSafe(async {
        for migration in [
            edgeagent_research_postgres::explanation_grants::MIGRATION,
            edgeagent_research_postgres::explanation_attempts::MIGRATION,
            edgeagent_research_postgres::explanation_completion::MIGRATION,
        ] {
            sqlx::raw_sql(migration).execute(&h.pool).await?;
        }
        let store = PostgresResearchStore::new(h.pool.clone());
        let base = prepared_owner_input("explanation")?;
        let original = store.commit(&base).await?;
        let decision = decode_decision(original.decision().ok_or("decision")?)?;
        let binding = Binding {
            anchor: Anchor { scope: "scope".into(), artifact: decision.outcome.clone(), decision: decision.digest()? },
            profile: "d".repeat(64),
            evidence: "e".repeat(64),
        };
        let validated = Allowlist::from_verified(original.record(), &decision)?.standard()?;
        let mut serving = super::explanation_grants::grant(Purpose::ExplanationServing);
        serving.valid_from_ms = 90;
        serving.request.anchor = binding.anchor.clone();
        let mut model = Grant { request: serving.request.clone(), ..serving.clone() };
        model.request.purpose = Purpose::ModelDisclosure;
        store.set_explanation_grant(&model, 1).await?;
        let intent = Intent { binding: binding.clone(), input_digest: "f".repeat(64), audience: model.request.audience.clone(), deadline_ms: 100 };
        let ReservationOutcome::Reserved(permit) = store.reserve_explanation_attempt(&intent, || 99).await? else { return Err("missing permit".into()); };
        let job = Job { anchor: binding.anchor.clone() };
        let fallback = || Completion::Fallback { explanation: &validated, reason: Fallback::UncertainAttempt };
        assert!(store.complete_explanation(base.request(), &binding, fallback(), Some(&serving), || 100, TIME).await.is_err());
        store.set_explanation_grant(&serving, 1).await?;
        assert!(store.complete_explanation(base.request(), &binding, fallback(), Some(&serving), || 99, TIME).await.is_err());
        store.recover_explanation_attempt(&job, || 100).await?;

        sqlx::raw_sql("CREATE FUNCTION explanation_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected failure'; END $$")
            .execute(&h.pool).await?;
        for (create, drop) in [
            ("CREATE TRIGGER explanation_fault BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION explanation_fault()", "DROP TRIGGER explanation_fault ON edgeagent_message_outbox"),
            ("CREATE CONSTRAINT TRIGGER explanation_fault AFTER INSERT ON edgeagent_explanation_completions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION explanation_fault()", "DROP TRIGGER explanation_fault ON edgeagent_explanation_completions"),
        ] {
            sqlx::raw_sql(create).execute(&h.pool).await?;
            assert!(store.complete_explanation(base.request(), &binding, fallback(), Some(&serving), || 100, TIME).await.is_err());
            assert_eq!(counts(&h).await?, (0, 0));
            sqlx::raw_sql(drop).execute(&h.pool).await?;
        }

        let restarted = PostgresResearchStore::new(h.reconnect().await?);
        let (first, duplicate) = tokio::join!(
            store.complete_explanation(base.request(), &binding, fallback(), Some(&serving), || 100, TIME),
            restarted.complete_explanation(base.request(), &binding, fallback(), Some(&serving), || 100, TIME),
        );
        let receipt = first?;
        assert_eq!(receipt.publication(), duplicate?.publication());
        assert!(matches!(restarted.recover_explanation_attempt(&job, || 151).await?, Recovery::Terminal));
        let recovered = restarted.recover_explanation(base.request(), &job).await?.ok_or("missing receipt")?;
        let retry = restarted.complete_explanation(base.request(), &binding, fallback(), None, || 100, "2026-10-31T00:00:00Z").await?;
        assert_eq!(receipt.bytes(), recovered.bytes());
        assert_eq!(receipt.publication(), recovered.publication());
        assert_eq!(receipt.publication(), retry.publication());
        assert_eq!(receipt.bytes(), retry.bytes());
        assert_eq!(Companion::decode(receipt.bytes())?.job().id()?, job.id()?);
        let companion_id = Companion::decode(receipt.bytes())?.id()?;
        assert!(store.current_explanation_read("scope", "explanation", &companion_id, &serving.request.audience, || 101).await?.is_none());
        assert!(store.current_explanation_read("scope", "explanation", &"b".repeat(64), &serving.request.audience, || 100).await?.is_none());
        assert!(store.current_explanation_read("scope", "explanation", &companion_id, "foreign", || 100).await?.is_none());
        let allowed = store.current_explanation_read("scope", "explanation", &companion_id, &serving.request.audience, || 100).await?.ok_or("missing exact owner input")?;
        assert_eq!(allowed.to_json()?, base.delivery().to_json()?);
        let revoked = Grant { revoked: true, ..serving.clone() };
        store.set_explanation_grant(&revoked, 2).await?;
        assert!(store.current_explanation_read("scope", "explanation", &companion_id, &serving.request.audience, || 100).await?.is_none());
        store.set_explanation_grant(&serving, 3).await?;
        store.append_lifecycle("scope", edgeagent_contracts::research::ResearchLifecycle {
            scope: "scope".into(),
            request_id: "explanation".into(),
            artifact: binding.anchor.artifact.clone(),
            revision: 1,
            effective_at_ms: 100,
            change: edgeagent_contracts::research::LifecycleChange::Withdrawn,
        }, TIME).await?;
        assert!(store.current_explanation_read("scope", "explanation", &companion_id, &serving.request.audience, || 100).await?.is_none());
        let late = Completion::Generated { explanation: &validated, permit: &permit };
        assert_eq!(store.complete_explanation(base.request(), &binding, late, Some(&serving), || 149, TIME).await.err().ok_or("late result accepted")?.kind(), Kind::Conflict);
        assert_eq!(counts(&h).await?, (1, 1));
        assert!(sqlx::query("DELETE FROM edgeagent_explanation_completions").execute(&h.pool).await.is_err());

        let second = prepared("scope", "refused", "refused", 110, 3)?;
        let retained = store.commit(&second).await?;
        let decision = decode_decision(retained.decision().ok_or("decision")?)?;
        let binding = Binding {
            anchor: Anchor { scope: "scope".into(), artifact: decision.outcome.clone(), decision: decision.digest()? },
            ..binding
        };
        let refusal = store.complete_explanation(second.request(), &binding, Completion::Refused(Refusal::DisclosureDenied), None, || 100, TIME).await?;
        assert!(refusal.publication().is_none());
        assert_eq!(counts(&h).await?, (2, 1));
        let refused_intent = Intent { binding, ..intent };
        assert!(matches!(store.reserve_explanation_attempt(&refused_intent, || 100).await?, ReservationOutcome::AlreadySpent));
        let third = prepared("scope", "generated", "generated", 110, 3)?;
        let retained = store.commit(&third).await?;
        let decision = decode_decision(retained.decision().ok_or("decision")?)?;
        let binding = Binding {
            anchor: Anchor { scope: "scope".into(), artifact: decision.outcome.clone(), decision: decision.digest()? },
            ..refused_intent.binding
        };
        model.request.anchor = binding.anchor.clone();
        serving.request.anchor = binding.anchor.clone();
        store.set_explanation_grant(&model, 1).await?;
        store.set_explanation_grant(&serving, 1).await?;
        let intent = Intent { binding: binding.clone(), deadline_ms: 150, ..refused_intent };
        let ReservationOutcome::Reserved(permit) = store.reserve_explanation_attempt(&intent, || 100).await? else {
            return Err("missing generated permit".into());
        };
        let validated = Allowlist::from_verified(retained.record(), &decision)?.standard()?;
        assert_eq!(store.complete_explanation(third.request(), &binding, Completion::Generated { explanation: &validated, permit: &permit }, Some(&serving), || 101, TIME).await.err().ok_or("expired completion accepted")?.kind(),Kind::Disabled);
        let generated = store.complete_explanation(third.request(), &binding, Completion::Generated { explanation: &validated, permit: &permit }, Some(&serving), || 100, TIME).await?;
        assert!(matches!(Companion::decode(generated.bytes())?.outcome, edgeagent_contracts::explanation::Outcome::Generated { .. }));
        let job = Job { anchor: binding.anchor };
        assert!(matches!(store.recover_explanation_attempt(&job, || 151).await?, Recovery::Terminal));
        assert_eq!(counts(&h).await?, (3, 2));
        assert_eq!(store.recover(base.delivery()).await?.ok_or("original receipt")?.bytes(), original.bytes());
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
