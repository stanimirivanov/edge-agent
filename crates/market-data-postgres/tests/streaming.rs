//! Invented streams and real PostgreSQL roles; no vendor transport or provider credentials.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned role/schema administration and injected database faults only."
)]
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/streaming.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use edgeagent_market_data_application::*;
use edgeagent_market_data_postgres::PostgresStreamingLedger as Ledger;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::*;

async fn next_start(h: &Harness) -> Result {
    // Test fixture only: no sleeps, counter refunds, policy renewal or production clock API.
    sqlx::query("UPDATE edgeagent_stream_accounts SET last_start_ms=NULL")
        .execute(&h.pool)
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_replicas_serialize_capacity_and_fence_unresolved_connections() -> Result {
    let h = Harness::new().await?;
    let (p1, r1) = h.worker().await?;
    let (p2, r2) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now, 2)?;
        let a = f.session(1)?;
        let b = f.session(2)?;
        let owner = Ledger::new(h.pool.clone());
        owner
            .configure_account(f.account()?, f.policy(1, 4)?, 0)
            .await?;
        owner.admit(&a, f.catalogs()).await?;
        owner.admit(&b, f.catalogs()).await?;
        let one = Ledger::new(p1.clone());
        let two = Ledger::new(p2.clone());
        let cancel = Cancellation::default();
        let (left, right) = tokio::join!(
            one.reserve(&a, f.catalogs(), &cancel),
            two.reserve(&b, f.catalogs(), &cancel)
        );
        let (session, other, connection, loser) = match (left, right) {
            (Ok(c), Err(e)) => (&a, &b, c, e),
            (Err(e), Ok(c)) => (&b, &a, c, e),
            _ => {
                return Err(
                    "capacity must admit exactly one independently authenticated replica".into(),
                );
            }
        };
        assert!(loser.kind() == BudgetErrorKind::Busy || is_lock_timeout(&loser));
        kind(
            two.reserve(other, f.catalogs(), &cancel).await,
            BudgetErrorKind::Busy,
        )?;
        assert_eq!(owner.status(&session.data().id).await?.usage.starts, 1);
        // Lost process/adapter and local cancellation release no durable capacity.
        drop(one);
        let restarted = Ledger::new(h.reconnect().await?);
        assert!(restarted.status(&session.data().id).await?.unresolved);
        kind(
            two.confirm_quiescent(f.account()?, &session.data().id, connection.generation())
                .await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            owner
                .confirm_quiescent(
                    f.account()?,
                    &session.data().id,
                    connection.generation() + 1,
                )
                .await,
            BudgetErrorKind::Conflict,
        )?;
        owner
            .confirm_quiescent(f.account()?, &session.data().id, connection.generation())
            .await?;
        owner
            .confirm_quiescent(f.account()?, &session.data().id, connection.generation())
            .await?;
        next_start(&h).await?;
        let replacement = two.reserve(session, f.catalogs(), &cancel).await?;
        assert!(replacement.generation() > connection.generation());
        assert_eq!(replacement.usage().starts, 2);
        kind(
            owner
                .confirm_quiescent(f.account()?, &session.data().id, connection.generation())
                .await,
            BudgetErrorKind::Conflict,
        )?;
        kind(
            two.charge(
                session,
                &connection,
                f.catalogs(),
                &cancel,
                StreamingArrival {
                    ordinal: 1,
                    bytes: 0,
                },
            )
            .await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            two.charge(
                other,
                &replacement,
                f.catalogs(),
                &cancel,
                StreamingArrival {
                    ordinal: 1,
                    bytes: 0,
                },
            )
            .await,
            BudgetErrorKind::Conflict,
        )?;
        owner.cancel(f.account()?, &session.data().id).await?;
        kind(
            two.charge(
                session,
                &replacement,
                f.catalogs(),
                &cancel,
                StreamingArrival {
                    ordinal: 1,
                    bytes: 0,
                },
            )
            .await,
            BudgetErrorKind::Denied,
        )?;
        assert!(owner.status(&session.data().id).await?.unresolved);
        kind(
            two.reserve(other, f.catalogs(), &cancel).await,
            BudgetErrorKind::Busy,
        )?;
        owner
            .confirm_quiescent(f.account()?, &session.data().id, replacement.generation())
            .await?;
        next_start(&h).await?;
        let other_connection = two.reserve(other, f.catalogs(), &cancel).await?;
        assert!(other_connection.generation() > replacement.generation());
        owner.withdraw_account(f.account()?).await?;
        kind(
            two.charge(
                other,
                &other_connection,
                f.catalogs(),
                &cancel,
                StreamingArrival {
                    ordinal: 1,
                    bytes: 0,
                },
            )
            .await,
            BudgetErrorKind::Denied,
        )?;
        assert!(owner.status(&other.data().id).await?.unresolved);
        owner
            .confirm_quiescent(
                f.account()?,
                &other.data().id,
                other_connection.generation(),
            )
            .await?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(p1, &r1).await?;
    h.drop_worker(p2, &r2).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_usage_survives_confirmation_loss_reconnect_and_transaction_failure() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async {
        let f=fixture::Fixture::new(h.now,1)?; let session=f.session(1)?;
        let owner=Ledger::new(h.pool.clone()); let worker=Ledger::new(pool.clone()); let cancel=Cancellation::default();
        let policy=f.policy(1,3)?;
        owner.configure_account(f.account()?,policy,1).await?;
        owner.admit(&session,f.catalogs()).await?;
        let connection=worker.reserve(&session,f.catalogs(),&cancel).await?;
        // Force an error after the frame update; no counters may partially commit.
        sqlx::raw_sql("CREATE FUNCTION fail_stream_frame() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic fault'; END $$; CREATE TRIGGER fail_stream_frame AFTER UPDATE ON edgeagent_stream_sessions FOR EACH ROW EXECUTE FUNCTION fail_stream_frame();")
            .execute(&h.pool).await?;
        kind(worker.charge(&session,&connection,f.catalogs(),&cancel,StreamingArrival { ordinal:1,bytes:0 }).await,BudgetErrorKind::Storage)?;
        assert_eq!(owner.status(&session.data().id).await?.usage.frames,0);
        sqlx::raw_sql("DROP TRIGGER fail_stream_frame ON edgeagent_stream_sessions; DROP FUNCTION fail_stream_frame();").execute(&h.pool).await?;
        let first=worker.charge(&session,&connection,f.catalogs(),&cancel,StreamingArrival { ordinal:1,bytes:0 }).await?;
        let repeated=worker.charge(&session,&connection,f.catalogs(),&cancel,StreamingArrival { ordinal:1,bytes:0 }).await?;
        assert!(repeated.repeated); assert_eq!(first.usage,repeated.usage);
        kind(worker.charge(&session,&connection,f.catalogs(),&cancel,StreamingArrival { ordinal:1,bytes:1 }).await,BudgetErrorKind::Conflict)?;
        owner.confirm_quiescent(f.account()?,&session.data().id,connection.generation()).await?;
        next_start(&h).await?;
        // Exact owner retries cannot reset spent account starts or session frames.
        owner.configure_account(f.account()?,policy,1).await?; owner.admit(&session,f.catalogs()).await?;
        let resumed=worker.reserve(&session,f.catalogs(),&cancel).await?;
        assert_eq!((resumed.usage().starts,resumed.usage().frames),(2,1));
        worker.charge(&session,&resumed,f.catalogs(),&cancel,StreamingArrival { ordinal:2,bytes:10 }).await?;
        let final_frame=worker.charge(&session,&resumed,f.catalogs(),&cancel,StreamingArrival { ordinal:3,bytes:10 }).await?;
        assert!(final_frame.within_limits && final_frame.usage.stopped);
        assert_eq!((final_frame.usage.frames,final_frame.usage.bytes),(3,20));
        assert!(worker.charge(&session,&resumed,f.catalogs(),&cancel,StreamingArrival { ordinal:3,bytes:10 }).await?.repeated);
        kind(worker.charge(&session,&resumed,f.catalogs(),&cancel,StreamingArrival { ordinal:4,bytes:0 }).await,BudgetErrorKind::Exhausted)?;
        assert!(owner.status(&session.data().id).await?.unresolved);
        owner.confirm_quiescent(f.account()?,&session.data().id,resumed.generation()).await?;
        let other=f.session(2)?; owner.admit(&other,f.catalogs()).await?;
        next_start(&h).await?;
        kind(worker.reserve(&other,f.catalogs(),&cancel).await,BudgetErrorKind::Exhausted)?;
        let starts:i64=sqlx::query_scalar("SELECT charged_starts FROM edgeagent_stream_accounts").fetch_one(&h.pool).await?;
        assert_eq!(starts,3); // Opening debit + two attempts, no release/refund.
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_owner_authority_expiry_and_failed_start_never_reset_allocations() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async {
        let f=fixture::Fixture::new(h.now,1)?; let session=f.session(1)?;
        let owner=Ledger::new(h.pool.clone()); let worker=Ledger::new(pool.clone()); let cancel=Cancellation::default();
        let mut policy=f.policy(1,3)?;
        policy.spacing_ms=fixture::nz(1_800_000)?;
        kind(worker.configure_account(f.account()?,policy,0).await,BudgetErrorKind::Denied)?;
        owner.configure_account(f.account()?,policy,0).await?;
        kind(worker.admit(&session,f.catalogs()).await,BudgetErrorKind::Denied)?;
        owner.admit(&session,f.catalogs()).await?;
        // Column ACLs are independent of table ACLs: none may reopen owner controls.
        for (privilege,relation) in [
            ("UPDATE(admission)","edgeagent_stream_admissions"),
            ("INSERT(session_id,account_id,admission,approved_session)","edgeagent_stream_admissions"),
            ("REFERENCES(admission)","edgeagent_stream_admissions"),
            ("INSERT(account_id)","edgeagent_stream_accounts"),
            ("UPDATE(connections)","edgeagent_stream_accounts"),
            ("REFERENCES(revision)","edgeagent_stream_accounts"),
        ] {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("GRANT {privilege} ON {relation} TO {role}"))).execute(&h.pool).await?;
            kind(worker.reserve(&session,f.catalogs(),&cancel).await,BudgetErrorKind::Denied)?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("REVOKE {privilege} ON {relation} FROM {role}"))).execute(&h.pool).await?;
        }
        let reviewer=format!("{role}_reviewer");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE ROLE {reviewer} NOLOGIN NOINHERIT; GRANT UPDATE(admission) ON edgeagent_stream_admissions TO {reviewer}; GRANT {reviewer} TO {role};"))).execute(&h.pool).await?;
        kind(worker.reserve(&session,f.catalogs(),&cancel).await,BudgetErrorKind::Denied)?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("REVOKE {reviewer} FROM {role}"))).execute(&h.pool).await?;
        // Temporary approvals must not shadow the early-bound protected read.
        sqlx::raw_sql("CREATE TEMP TABLE edgeagent_stream_admissions(session_id text, account_id text, admission bytea, withdrawn bool);")
            .execute(&pool).await?;
        let changed={ let mut data=f.data(1)?; data.limits.frames=fixture::nz(4)?; f.seal(data)? };
        kind(owner.admit(&changed,f.catalogs()).await,BudgetErrorKind::Conflict)?;
        sqlx::query("INSERT INTO pg_temp.edgeagent_stream_admissions VALUES($1,$2,$3,FALSE)")
            .bind(changed.data().id.as_str()).bind(format!("{:032x}",f.account()?.get())).bind(changed.bytes()).execute(&pool).await?;
        kind(worker.reserve(&changed,f.catalogs(),&cancel).await,BudgetErrorKind::Denied)?;
        sqlx::raw_sql("DROP TABLE pg_temp.edgeagent_stream_admissions").execute(&pool).await?;
        // A failed session update must roll back both account debit and global fence.
        sqlx::raw_sql("CREATE FUNCTION fail_stream_start() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic fault'; END $$; CREATE TRIGGER fail_stream_start BEFORE UPDATE ON edgeagent_stream_sessions FOR EACH ROW EXECUTE FUNCTION fail_stream_start();")
            .execute(&h.pool).await?;
        kind(worker.reserve(&session,f.catalogs(),&cancel).await,BudgetErrorKind::Storage)?;
        let counters:(i64,i64)=sqlx::query_as("SELECT charged_starts,generation FROM edgeagent_stream_accounts").fetch_one(&h.pool).await?;
        assert_eq!(counters,(0,0));
        sqlx::raw_sql("DROP TRIGGER fail_stream_start ON edgeagent_stream_sessions; DROP FUNCTION fail_stream_start();").execute(&h.pool).await?;
        let connection=worker.reserve(&session,f.catalogs(),&cancel).await?;
        owner.confirm_quiescent(f.account()?,&session.data().id,connection.generation()).await?;
        kind(worker.reserve(&session,f.catalogs(),&cancel).await,BudgetErrorKind::RateLimited)?;
        assert_eq!(owner.status(&session.data().id).await?.usage.starts,1);
        next_start(&h).await?;
        let connection=worker.reserve(&session,f.catalogs(),&cancel).await?;
        let revoked=f.revoked()?;
        kind(worker.charge(&session,&connection,AuthorityCatalogs { rights:&revoked,providers:&f.providers },&cancel,StreamingArrival { ordinal:1,bytes:0 }).await,BudgetErrorKind::Denied)?;
        cancel.cancel();
        kind(worker.charge(&session,&connection,f.catalogs(),&cancel,StreamingArrival { ordinal:1,bytes:0 }).await,BudgetErrorKind::Cancelled)?;
        // Explicit fixture expiry: no sleep and no production override to reclaim a slot.
        sqlx::query("UPDATE edgeagent_stream_accounts SET expires_ms=0").execute(&h.pool).await?;
        kind(worker.charge(&session,&connection,f.catalogs(),&Cancellation::default(),StreamingArrival { ordinal:1,bytes:0 }).await,BudgetErrorKind::Denied)?;
        assert!(owner.status(&session.data().id).await?.unresolved);
        owner.confirm_quiescent(f.account()?,&session.data().id,connection.generation()).await?;
        let mut changed_policy=policy; changed_policy.starts=fixture::nz(4)?;
        kind(owner.configure_account(f.account()?,changed_policy,0).await,BudgetErrorKind::Conflict)?;
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
