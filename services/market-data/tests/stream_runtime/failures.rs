use super::*;
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn offline_runtime_sticky_failures_and_revocation_never_grant_readiness() -> Result {
    let h = db::Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let operation = async {
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        let clock = Clock::new(h.now)?;
        for mode in 0..3 {
            let f = Fixture::new(h.now - 100, mode + 1)?;
            let plan = f.base.plan()?;
            f.install(&owner).await?;
            let store = Store::new(None)?;
            let queue = if mode == 2 { 1 } else { 8 };
            let mut runtime = OfflineStreamRuntime::begin(
                vec![f.source(&plan, queue)?],
                worker.clone(),
                &store,
                &clock,
                Cancellation::default(),
                limits(1)?,
            )
            .await?;
            approve(&owner, &runtime, &[&f]).await?;
            let mut first = f.trade(1, 0, 1, 1000)?;
            if mode == 0
                && let edgeagent_market_data_http::IexValue::Trade { conditions, .. } =
                    &mut first.value
            {
                *conditions = vec!["?".into()];
            }
            let records = if mode == 0 {
                vec![first]
            } else {
                vec![first, f.trade(1, 1, if mode == 1 { 1 } else { 2 }, 2000)?]
            };
            let expected = match mode {
                0 => OfflineStreamError::Semantics,
                1 => OfflineStreamError::Continuity,
                _ => OfflineStreamError::Overload,
            };
            assert_eq!(
                runtime.run(vec![script(vec![frame(1, records)])]).await,
                Err(expected)
            );
            assert!(
                runtime
                    .ready(
                        &f.policy(ContinuityRequirement::Snapshots)?,
                        &[f.candidate(1)?]
                    )
                    .await
                    .is_err()
            );
            assert!(
                worker
                    .recovery_state(&f.session, f.base.catalogs())
                    .await?
                    .incident
                    .is_some()
            );
            assert!(worker.status(&f.session.data().id).await?.unresolved);
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_stream_publications WHERE session_id=$1 AND published").bind(f.session.data().id.as_str()).fetch_one(&h.pool).await?;
            assert_eq!(count, 0);
        }
        let f = Fixture::new(h.now - 100, 4)?;
        let plan = f.base.plan()?;
        f.install(&owner).await?;
        let store = Store::new(Some(f.session.data().id.clone()))?;
        let mut bad = limits(1)?;
        bad.run_ms = NonZeroU32::new(60001).ok_or("limit")?;
        assert_eq!(
            OfflineStreamRuntime::begin(
                vec![f.source(&plan, 8)?],
                worker.clone(),
                &store,
                &clock,
                Cancellation::default(),
                bad
            )
            .await
            .err(),
            Some(OfflineStreamError::Configuration)
        );
        let mut runtime = OfflineStreamRuntime::begin(
            vec![f.source(&plan, 8)?],
            worker.clone(),
            &store,
            &clock,
            Cancellation::default(),
            limits(1)?,
        )
        .await?;
        approve(&owner, &runtime, &[&f]).await?;
        let revoke = async {
            store.entered.notified().await;
            owner
                .withdraw_stream_data(f.session.account(), &f.session.data().id)
                .await?;
            store.release.notify_one();
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (run, revoke) = tokio::join!(
            runtime.run(vec![script(vec![frame(1, vec![f.trade(1, 0, 1, 1000)?])])]),
            revoke
        );
        revoke?;
        assert!(run.is_err());
        assert!(
            runtime
                .ready(
                    &f.policy(ContinuityRequirement::Snapshots)?,
                    &[f.candidate(1)?]
                )
                .await
                .is_err()
        );
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM edgeagent_stream_publications WHERE session_id=$1 AND published),(SELECT count(*) FROM edgeagent_message_outbox)").bind(f.session.data().id.as_str()).fetch_one(&h.pool).await?;
        assert_eq!(counts, (0, 0));
        Ok(())
    };
    let result = AssertUnwindSafe(operation).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
