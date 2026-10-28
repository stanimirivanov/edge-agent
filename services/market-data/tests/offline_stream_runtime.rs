//! Real restricted PostgreSQL identities, synthetic frames and in-memory immutable objects only.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated owned-schema observations and explicit synthetic fault injection."
)]
#[allow(dead_code)]
#[path = "../../../crates/market-data-postgres/tests/support/mod.rs"]
mod db;
#[allow(dead_code)]
#[path = "stream_runtime/fixture.rs"]
mod fixture;
use fixture::*;
#[path = "stream_runtime/failures.rs"]
mod failures;
use edgeagent_market_data::offline_streaming::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_postgres::{PostgresStreamingLedger as Ledger, StreamRuntimePhase};
use futures_util::FutureExt;
use std::{
    collections::VecDeque, num::NonZeroU32, panic::AssertUnwindSafe, sync::atomic::Ordering,
    time::Duration,
};

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn offline_runtime_requires_approval_and_publishes_exact_ready_evidence() -> Result {
    let h = db::Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let operation = async {
        let f = Fixture::members(h.now - 100, 1, 2)?;
        let selection = f.base.plan()?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        f.install(&owner).await?;
        let clock = Clock::new(h.now)?;
        let store = Store::new(None)?;
        let cancel = Cancellation::default();
        let mut runtime = OfflineStreamRuntime::begin(
            vec![f.source(&selection, 8)?],
            worker.clone(),
            &store,
            &clock,
            cancel.clone(),
            limits(1)?,
        )
        .await?;
        assert!(
            runtime
                .ready(
                    &f.policy(ContinuityRequirement::Snapshots)?,
                    &[f.candidate(11)?]
                )
                .await
                .is_err()
        );
        assert_eq!(
            runtime.run(vec![VecDeque::new()]).await,
            Err(OfflineStreamError::Authority)
        );
        assert_eq!(worker.status(&f.session.data().id).await?.usage.starts, 0);
        approve(&owner, &runtime, &[&f]).await?;
        runtime
            .run(vec![script(vec![
                frame(
                    1,
                    vec![f.trade(1, 0, 11, 1000)?, f.trade_for(2, 1, 1, 12, 1000)?],
                ),
                frame(2, vec![f.trade(2, 0, 11, 1000)?]),
            ])])
            .await?;
        let policy = f.policy(ContinuityRequirement::Snapshots)?;
        let ready = runtime.ready(&policy, &[f.candidate(11)?]).await?;
        assert_eq!(
            ready
                .observations()
                .first()
                .ok_or("ready")?
                .times()
                .received_at(),
            PreciseInstant::from_millis(at(f.base.ms + 1)?)
        );
        drop(ready);
        assert!(
            runtime
                .ready(
                    &f.policy(ContinuityRequirement::Contiguous)?,
                    &[f.candidate(11)?]
                )
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM edgeagent_stream_publications WHERE published",
        )
        .fetch_one(&h.pool)
        .await?;
        assert_eq!(count, 3);
        clock.set(h.now + 5000)?;
        assert_eq!(
            runtime.ready(&policy, &[f.candidate(11)?]).await.err(),
            Some(OfflineStreamError::NotReady)
        );
        clock.set(h.now)?;
        // A cached domain tracker cannot bypass expiry/retirement of its original receipt.
        let mut first = None;
        for entry in worker
            .stream_history(&f.session, f.base.catalogs(), 16)
            .await?
        {
            let p = worker
                .recover_segment(&f.session, &entry.publication, f.base.catalogs())
                .await?
                .ok_or("capture")?;
            if p.segment().data().frames[1].as_str() == "1"
                && p.segment().data().scope.instrument.as_str() == format!("{:032x}", 1)
            {
                first = Some(entry);
            }
        }
        let first = first.ok_or("history")?;
        sqlx::query("UPDATE edgeagent_stream_publications SET delete_ms=0 WHERE publication_id=$1")
            .bind(first.publication.as_str())
            .execute(&h.pool)
            .await?;
        owner
            .retire_stream_segment(
                f.session.account(),
                &f.session.data().id,
                &first.publication,
            )
            .await?;
        assert!(runtime.ready(&policy, &[f.candidate(11)?]).await.is_err());
        let reads = store.gets.load(Ordering::SeqCst);
        assert!(
            OfflineStreamRuntime::begin(
                vec![f.source(&selection, 8)?],
                worker.clone(),
                &store,
                &clock,
                Cancellation::default(),
                limits(1)?
            )
            .await
            .is_err()
        );
        assert_eq!(store.gets.load(Ordering::SeqCst), reads);
        cancel.cancel();
        assert!(runtime.ready(&policy, &[f.candidate(11)?]).await.is_err());
        Ok(())
    };
    let result = AssertUnwindSafe(operation).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn offline_runtime_isolates_slow_sources_with_bounded_fair_dispatch() -> Result {
    let h = db::Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let operation = async {
        let a = Fixture::new(h.now - 100, 1)?;
        let b = Fixture::new(h.now - 100, 2)?;
        let pa = a.base.plan()?;
        let pb = b.base.plan()?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        a.install(&owner).await?;
        b.install(&owner).await?;
        let clock = Clock::new(h.now)?;
        let store = Store::new(Some(a.session.data().id.clone()))?;
        let mut runtime = OfflineStreamRuntime::begin(
            vec![a.source(&pa, 8)?, b.source(&pb, 8)?],
            worker,
            &store,
            &clock,
            Cancellation::default(),
            limits(2)?,
        )
        .await?;
        approve(&owner, &runtime, &[&a, &b]).await?;
        let observer = async {
            store.entered.notified().await;
            tokio::time::timeout(Duration::from_secs(10),async {
                loop {
                    let count:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_stream_publications WHERE session_id=$1 AND published").bind(b.session.data().id.as_str()).fetch_one(&h.pool).await?;
                    if count==1 {break;}
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
            }).await??;
            store.release.notify_one();
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (run, observe) = tokio::join!(
            runtime.run(vec![
                script(vec![frame(1, vec![a.trade(1, 0, 1, 1000)?])]),
                script(vec![frame(1, vec![b.trade(1, 0, 2, 2000)?])])
            ]),
            observer
        );
        run?;
        observe?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM edgeagent_stream_publications WHERE published",
        )
        .fetch_one(&h.pool)
        .await?;
        assert_eq!(count, 2);
        assert!(
            runtime
                .ready(
                    &a.policy(ContinuityRequirement::Snapshots)?,
                    &[a.candidate(1)?]
                )
                .await
                .is_ok()
        );
        Ok(())
    };
    let result = AssertUnwindSafe(operation).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn offline_runtime_restart_recovers_exact_native_intent_and_fences_old_workers() -> Result {
    let h = db::Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let operation = async {
        let f = Fixture::new(h.now - 100, 1)?;
        let p = f.base.plan()?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        f.install(&owner).await?;
        let clock = Clock::new(h.now)?;
        let store = Store::new(Some(f.session.data().id.clone()))?;
        let mut old = OfflineStreamRuntime::begin(
            vec![f.source(&p, 8)?],
            worker.clone(),
            &store,
            &clock,
            Cancellation::default(),
            limits(1)?,
        )
        .await?;
        approve(&owner, &old, &[&f]).await?;
        let (_, witness) = old.restores().next().ok_or("witness")?;
        let old_witness =
            StreamRestore::from_committed(witness.identity().0.clone(), witness.identity().1)?;
        let old_worker = worker.fenced(&old_witness, StreamRuntimePhase::Running);
        {
            let run = old.run(vec![script(vec![frame(
                1,
                vec![f.trade(1, 0, 9007199254740993, 1000)?],
            )])]);
            tokio::pin!(run);
            tokio::select! {result=&mut run=>{result?;return Err("worker completed before controlled state loss".into());}, _=store.entered.notified()=>{}}
        } // Drop the entire worker future after PUT; no async Drop or implicit retry.
        assert_eq!(
            old.run(vec![VecDeque::new()]).await,
            Err(OfflineStreamError::Configuration)
        );
        assert!(old.lane_status(FeedId::new(1)?)?.unresolved);
        assert_eq!(
            old.run(vec![VecDeque::new()]).await,
            Err(OfflineStreamError::Configuration)
        );
        assert!(old.lane_status(FeedId::new(1)?)?.unresolved);
        let original = worker
            .pending_segment(&f.session, InstrumentId::new(1)?, f.base.catalogs())
            .await?
            .ok_or("pending")?;
        let before = worker.status(&f.session.data().id).await?;
        let mut restored = OfflineStreamRuntime::begin(
            vec![f.source(&p, 8)?],
            worker.clone(),
            &store,
            &clock,
            Cancellation::default(),
            limits(1)?,
        )
        .await?;
        assert!(
            old_worker
                .commit_segment(
                    &f.session,
                    &original,
                    f.base.catalogs(),
                    &Cancellation::default()
                )
                .await
                .is_err()
        );
        assert!(
            owner
                .approve_stream_restore(&f.session, &old_witness, f.base.catalogs())
                .await
                .is_err()
        );
        let actual = worker
            .recover_segment(&f.session, original.id(), f.base.catalogs())
            .await?
            .ok_or("receipt")?;
        assert_eq!(actual.bytes()?, original.bytes()?);
        assert_eq!(
            before.usage,
            worker.status(&f.session.data().id).await?.usage
        );
        approve(&owner, &restored, &[&f]).await?;
        restored.run(vec![VecDeque::new()]).await?;
        assert!(
            restored
                .ready(
                    &f.policy(ContinuityRequirement::Snapshots)?,
                    &[f.candidate(9007199254740993)?]
                )
                .await
                .is_err()
        ); // Restore is not a fresh activity pulse.
        owner
            .confirm_quiescent(f.session.account(), &f.session.data().id, before.generation)
            .await?;
        let mut active = OfflineStreamRuntime::begin(
            vec![f.source(&p, 8)?],
            worker.clone(),
            &store,
            &clock,
            Cancellation::default(),
            limits(1)?,
        )
        .await?;
        approve(&owner, &active, &[&f]).await?;
        active.run(vec![script(vec![frame(2, vec![])])]).await?;
        let policy = f.policy(ContinuityRequirement::Snapshots)?;
        let ready = active
            .ready(&policy, &[f.candidate(9007199254740993)?])
            .await?;
        assert_eq!(
            ready
                .observations()
                .first()
                .ok_or("ready")?
                .times()
                .received_at(),
            PreciseInstant::from_millis(at(f.base.ms + 1)?)
        );
        drop(ready);
        let mut small = limits(1)?;
        small.history_bytes = NonZeroU32::new(1).ok_or("limit")?;
        let reads = store.gets.load(Ordering::SeqCst);
        assert!(
            OfflineStreamRuntime::begin(
                vec![f.source(&p, 8)?],
                worker,
                &store,
                &clock,
                Cancellation::default(),
                small
            )
            .await
            .is_err()
        );
        assert_eq!(store.gets.load(Ordering::SeqCst), reads);
        Ok(())
    };
    let result = AssertUnwindSafe(operation).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
