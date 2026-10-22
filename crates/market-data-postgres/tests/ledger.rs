//! Real transactions in owned schemas; no provider request is executed.
#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema administration and fault injection."
)]
mod support;
use edgeagent_market_data_application::*;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::*;

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn duplicate_and_conflicting_admission_preserve_original_deadline_and_bytes() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let f = Case::new(h.now, 1)?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let record = f.record()?;
        let c = Cancellation::default();
        let authority = f.catalogs().admit(&f.request, f.request.admitted_at, &c)?;
        let (one, two) = tokio::join!(
            ledger.admit(&record, &authority),
            ledger.admit(&record, &authority)
        );
        assert_eq!(one?, two?);
        ledger.begin_attempt(record.command(), 1).await?;
        assert_eq!(ledger.begin_attempt(record.command(), 1).await?.attempts, 1);
        let again = ledger.admit(&record, &authority).await?;
        assert_eq!(again, record);
        assert_eq!(ledger.load(record.command()).await?.1.attempts, 1);
        let mut changed = Case::new(h.now, 1)?;
        changed.request.budget.calls = std::num::NonZeroU32::new(11).ok_or("nonzero")?;
        let changed_record = changed.record()?;
        let grant = changed
            .catalogs()
            .admit(&changed.request, changed.request.admitted_at, &c)?;
        kind(
            ledger.admit(&changed_record, &grant).await,
            BudgetErrorKind::Conflict,
        )?;
        let (stored, state) = ledger.load(record.command()).await?;
        assert_eq!(stored.bytes(), record.bytes());
        assert_eq!(state.attempts, 1);
        assert_eq!(stored.request().deadline, record.request().deadline);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn competing_replicas_restart_and_lost_completion_preserve_permits_and_charges() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let f = Case::new(h.now, 1)?;
        let other = Case::new(h.now, 2)?;
        let r = f.record()?;
        let s = other.record()?;
        let left = PostgresAcquisitionLedger::new(h.pool.clone());
        let pool = h.reconnect().await?;
        let right = PostgresAcquisitionLedger::new(pool.clone());
        let cancel = Cancellation::default();
        left.admit(
            &r,
            &f.catalogs()
                .admit(&f.request, f.request.admitted_at, &cancel)?,
        )
        .await?;
        right
            .admit(
                &s,
                &other
                    .catalogs()
                    .admit(&other.request, other.request.admitted_at, &cancel)?,
            )
            .await?;
        left.begin_attempt(r.command(), 1).await?;
        right.begin_attempt(s.command(), 1).await?;
        let (a, b) = tokio::join!(
            left.start_call(r.command(), None),
            right.start_call(s.command(), None)
        );
        let (permit, loser) = match (a, b) {
            (Ok(p), Err(e)) | (Err(e), Ok(p)) => (p, e),
            _ => return Err("expected one exclusive permit".into()),
        };
        assert_eq!(loser.kind(), BudgetErrorKind::Busy);
        drop(left);
        drop(right);
        pool.close().await;
        let pool = h.reconnect().await?;
        let restarted = PostgresAcquisitionLedger::new(pool.clone());
        let (record, state) = restarted.load(permit.command()).await?;
        assert_eq!(state.calls, 1);
        assert_eq!(state.charged_bytes, 1_048_576);
        assert_eq!(state.pending.as_ref(), Some(&permit));
        kind(
            restarted.begin_attempt(permit.command(), 2).await,
            BudgetErrorKind::Busy,
        )?;
        let finished = CallCompletion::Observed {
            decoded_bytes: 7,
            retry_after_ms: None,
        };
        restarted.complete_call(&permit, finished).await?; // Model a lost completion response.
        kind(
            restarted.start_call(permit.command(), None).await,
            BudgetErrorKind::RateLimited,
        )?;
        h.age_rate_history().await?;
        let next = restarted.start_call(permit.command(), None).await?;
        let before = restarted.load(permit.command()).await?.1;
        assert_eq!(before.calls, 2);
        assert_eq!(before.charged_bytes, 1_048_583);
        restarted.complete_call(&permit, finished).await?;
        assert_eq!(restarted.load(permit.command()).await?.1, before);
        kind(
            restarted
                .complete_call(&permit, CallCompletion::QuiescedUnknown)
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let foreign = if record.command() == r.command() {
            s.command()
        } else {
            r.command()
        };
        kind(
            restarted.start_call(foreign, None).await,
            BudgetErrorKind::Busy,
        )?;
        restarted
            .complete_call(&next, CallCompletion::QuiescedUnknown)
            .await?;
        assert_eq!(
            restarted.load(permit.command()).await?.1.charged_bytes,
            1_048_583
        );
        pool.close().await;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn expired_cancelled_and_corrupt_state_fail_closed_without_partial_mutation() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let cancel = Cancellation::default();
        let old = Case::new(172_800_000, 9)?;
        let old_record = old.record()?;
        kind(
            ledger
                .admit(
                    &old_record,
                    &old.catalogs()
                        .admit(&old.request, old.request.admitted_at, &cancel)?,
                )
                .await,
            BudgetErrorKind::Deadline,
        )?;
        let f = Case::new(h.now, 1)?;
        let record = f.record()?;
        ledger
            .admit(
                &record,
                &f.catalogs()
                    .admit(&f.request, f.request.admitted_at, &cancel)?,
            )
            .await?;
        ledger.begin_attempt(record.command(), 1).await?;
        let permit = ledger.start_call(record.command(), None).await?;
        let before = ledger.load(record.command()).await?.1;
        let saved: Vec<u8> =
            sqlx::query_scalar("SELECT budget FROM edgeagent_acquisition_accounts")
                .fetch_one(&h.pool)
                .await?;
        sqlx::query("UPDATE edgeagent_acquisition_accounts SET budget=$1")
            .bind(b"{}".as_slice())
            .execute(&h.pool)
            .await?;
        kind(
            ledger.cancel(record.command()).await,
            BudgetErrorKind::Invalid,
        )?;
        assert_eq!(ledger.load(record.command()).await?.1, before);
        sqlx::query("UPDATE edgeagent_acquisition_accounts SET budget=$1")
            .bind(saved)
            .execute(&h.pool)
            .await?;
        let state = ledger.cancel(record.command()).await?;
        assert!(state.cancelled);
        assert_eq!(state.pending.as_ref(), Some(&permit));
        kind(
            ledger.start_call(record.command(), None).await,
            BudgetErrorKind::Cancelled,
        )?;
        ledger
            .complete_call(&permit, CallCompletion::QuiescedUnknown)
            .await?;
        assert_eq!(
            ledger.load(record.command()).await?.1.charged_bytes,
            1_048_576
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
