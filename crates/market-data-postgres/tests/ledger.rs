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
        // Contending admissions may hit the bounded database lock timeout.
        // Both calls have returned: retry only that known rollback, never
        // conceal arbitrary storage failures or reset the admitted identity.
        for outcome in [one, two] {
            let admitted = match outcome {
                Err(error) if is_lock_timeout(&error) => ledger.admit(&record, &authority).await?,
                result => result?,
            };
            assert_eq!(admitted, record);
        }
        // Force the recovery branch even on fast hosts. Release the blocker
        // before checking the outcome so an unexpected result cannot hold it.
        let mut blocker = h.pool.begin().await?;
        sqlx::query("SELECT scope FROM edgeagent_evidence_scopes FOR UPDATE")
            .execute(&mut *blocker)
            .await?;
        let blocked = ledger.admit(&record, &authority).await;
        blocker.rollback().await?;
        let error = blocked.err().ok_or("expected bounded lock timeout")?;
        assert!(is_lock_timeout(&error));
        assert_eq!(ledger.admit(&record, &authority).await?, record);
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
        // Reconnect and fault checks must not race a 15-second wall-clock window
        // on a loaded host. Keep a two-minute window within the five-minute
        // command deadline; age_rate_history below
        // explicitly releases it without sleeping or changing production time.
        let r = f.record_with_spacing(120000)?;
        let s = other.record_with_spacing(120000)?;
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
        h.configure_credits().await?;
        let (a, b) = tokio::join!(
            left.start_call(r.command(), None),
            right.start_call(s.command(), None)
        );
        let (permit, loser) = match (a, b) {
            (Ok(p), Err(e)) | (Err(e), Ok(p)) => (p, e),
            _ => return Err("expected one exclusive permit".into()),
        };
        assert!(
            loser.kind() == BudgetErrorKind::Busy || is_lock_timeout(&loser),
            "unexpected contender failure: {loser:?}"
        );
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
        h.configure_credits().await?;
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

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn daily_credits_commit_with_permits_and_survive_contention_rollback_and_restart() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let left = PostgresAcquisitionLedger::new(h.pool.clone());
        let mut commands = Vec::new();
        for id in [1, 2] {
            let f = Case::new(h.now, id)?;
            let r = f.record()?;
            left.admit(&r, &f.catalogs().admit(&f.request, f.request.admitted_at, &Cancellation::default())?).await?;
            left.begin_attempt(r.command(), 1).await?;
            commands.push(r.command().clone());
        }
        let (a, b) = match commands.as_slice() { [a, b] => (a, b), _ => return Err("two commands".into()) };
        let account = ProviderAccountId::new(1)?;
        assert_eq!(left.daily_credit_usage(account).await?, None);
        kind(left.start_call(a, None).await, BudgetErrorKind::Denied)?;
        let policy = DailyCreditPolicy::new(1, 5, h.now + 86_400_000, &[(edgeagent_market_data_domain::ReferenceId::new(1)?, 2)])?;
        left.configure_daily_credits(account, policy.clone(), 3).await?;
        // Fail the second write: neither command reservation nor account credits may commit.
        sqlx::raw_sql("CREATE FUNCTION reject_account() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic account write failure'; END $$;
            CREATE TRIGGER reject_account BEFORE UPDATE ON edgeagent_acquisition_accounts FOR EACH ROW EXECUTE FUNCTION reject_account();")
            .execute(&h.pool).await?;
        kind(left.start_call(a, None).await, BudgetErrorKind::Storage)?;
        assert_eq!(left.load(a).await?.1.calls, 0);
        assert_eq!(left.daily_credit_usage(account).await?.ok_or("usage")?.charged, 3);
        sqlx::raw_sql("DROP TRIGGER reject_account ON edgeagent_acquisition_accounts; DROP FUNCTION reject_account();").execute(&h.pool).await?;
        let pool = h.reconnect().await?;
        let right = PostgresAcquisitionLedger::new(pool.clone());
        let (one, two) = tokio::join!(left.start_call(a, None), right.start_call(b, None));
        let permit = match (one, two) {
            // The configured SQL lock timeout is also a confirmed aborted loser;
            // retain all subsequent exact credit/permit/restart assertions.
            (Ok(p), Err(e)) | (Err(e), Ok(p)) if e.kind() == BudgetErrorKind::Busy || is_lock_timeout(&e) => p,
            (a,b) => return Err(format!("expected one committed permit and one fenced competitor; got {a:?}, {b:?}").into()),
        };
        assert_eq!(right.daily_credit_usage(account).await?.ok_or("usage")?.charged, 5);
        drop(right);
        pool.close().await;
        let pool = h.reconnect().await?;
        let restarted = PostgresAcquisitionLedger::new(pool.clone());
        assert_eq!(restarted.configure_daily_credits(account, policy.clone(), 3).await?.charged, 5);
        kind(restarted.configure_daily_credits(account, policy, 0).await, BudgetErrorKind::Conflict)?;
        assert_eq!(restarted.load(permit.command()).await?.1.pending.as_ref(), Some(&permit));
        restarted.complete_call(&permit, CallCompletion::QuiescedUnknown).await?;
        restarted.complete_call(&permit, CallCompletion::QuiescedUnknown).await?;
        h.age_rate_history().await?;
        let other = if permit.command() == a { b } else { a };
        kind(restarted.start_call(other, None).await, BudgetErrorKind::Exhausted)?;
        assert_eq!(restarted.load(other).await?.1.calls, 0);
        assert_eq!(restarted.daily_credit_usage(account).await?.ok_or("usage")?.charged, 5);
        pool.close().await;
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
