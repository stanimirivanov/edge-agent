use super::*;
use edgeagent_contracts::explanation::{Binding, Job};
use edgeagent_research_application::explanation_context::Purpose;
use edgeagent_research_postgres::explanation_attempts::{Intent, Recovery, ReservationOutcome};

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn explanation_attempts_spend_once_and_fence_ambiguous_recovery() -> Result {
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
        for (index, artifact) in ["a".repeat(64), "c".repeat(64)].into_iter().enumerate() {
            let mut grant = super::explanation_grants::grant(Purpose::ModelDisclosure);
            grant.request.anchor.artifact = artifact;
            let intent = Intent {
                binding: Binding {
                    anchor: grant.request.anchor.clone(),
                    profile: "d".repeat(64),
                    evidence: "e".repeat(64),
                },
                input_digest: "f".repeat(64),
                audience: grant.request.audience.clone(),
                deadline_ms: 150,
            };
            let job = Job {
                anchor: intent.binding.anchor.clone(),
            };
            assert!(matches!(
                store.recover_explanation_attempt(&job, || 100).await?,
                Recovery::Missing
            ));
            assert_eq!(
                store
                    .reserve_explanation_attempt(&intent, || 100)
                    .await
                    .err()
                    .ok_or("missing grant")?
                    .kind(),
                Kind::Disabled
            );
            store.set_explanation_grant(&grant, 1).await?;
            let beyond_grant = Intent {
                deadline_ms: 201,
                ..intent.clone()
            };
            assert_eq!(
                store
                    .reserve_explanation_attempt(&beyond_grant, || 100)
                    .await
                    .err()
                    .ok_or("deadline exceeds grant")?
                    .kind(),
                Kind::Disabled
            );
            let competing = PostgresResearchStore::new(h.reconnect().await?);
            let (first, second) = tokio::join!(
                store.reserve_explanation_attempt(&intent, || 100),
                competing.reserve_explanation_attempt(&intent, || 100),
            );
            let permit = match (first?, second?) {
                (ReservationOutcome::Reserved(p), ReservationOutcome::AlreadySpent)
                | (ReservationOutcome::AlreadySpent, ReservationOutcome::Reserved(p)) => p,
                _ => return Err("exactly one worker must obtain the permit".into()),
            };
            assert_eq!(permit.grant_revision(), 1);
            let conflict = Intent {
                input_digest: "9".repeat(64),
                ..intent.clone()
            };
            assert_eq!(
                store
                    .reserve_explanation_attempt(&conflict, || 101)
                    .await
                    .err()
                    .ok_or("conflicting intent")?
                    .kind(),
                Kind::Conflict
            );
            assert_eq!(permit.intent().input_digest, intent.input_digest);
            if index == 1 {
                // Simulate a returned response whose retaining transaction is lost.
                let mut tx = h.pool.begin().await?;
                PostgresResearchStore::fence_explanation_result(&mut tx, &permit, || 149).await?;
                tx.rollback().await?;
            }

            // The other case abandons the permit before touching a result.
            // Both interruptions leave the same durable spent state.
            let restarted = PostgresResearchStore::new(h.reconnect().await?);
            assert!(matches!(
                restarted.recover_explanation_attempt(&job, || 149).await?,
                Recovery::Pending { deadline_ms: 150 }
            ));
            let mut tx = h.pool.begin().await?;
            assert!(
                PostgresResearchStore::fence_explanation_result(&mut tx, &permit, || 150)
                    .await
                    .is_err()
            );
            tx.rollback().await?;
            store
                .set_explanation_grant(
                    &edgeagent_research_application::explanation_context::Grant {
                        revoked: true,
                        ..grant
                    },
                    2,
                )
                .await?;
            for _ in 0..2 {
                let Recovery::Fallback(spent) =
                    restarted.recover_explanation_attempt(&job, || 150).await?
                else {
                    return Err("expired attempt must require fallback".into());
                };
                assert_eq!(spent.intent().input_digest, intent.input_digest);
                assert_eq!(spent.intent().deadline_ms, 150);
                assert_eq!(spent.grant_revision(), 1);
            }
            assert!(matches!(
                restarted
                    .reserve_explanation_attempt(&intent, || 101)
                    .await?,
                ReservationOutcome::AlreadySpent
            ));
            let mut tx = h.pool.begin().await?;
            assert!(
                PostgresResearchStore::fence_explanation_result(&mut tx, &permit, || 149)
                    .await
                    .is_err()
            );
            tx.rollback().await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_explanation_attempts")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 2);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
