//! Real transaction proofs in owned schemas; no provider or broker is contacted.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated test schema setup and rollback fault injection."
)]
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/publication.rs"]
mod fixtures;
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use fixtures::{Access, Storage, delivery, prepare};
use futures_util::FutureExt;
use std::{panic::AssertUnwindSafe, sync::atomic::Ordering};
use support::*;
async fn admit(h: &Harness, id: u128) -> Result<(Case, AdmissionRecord)> {
    let case = Case::new(h.now, id)?;
    let record = case.record()?;
    PostgresAcquisitionLedger::new(h.pool.clone())
        .admit(
            &record,
            &case.catalogs().admit(
                &case.request,
                case.request.admitted_at,
                &Cancellation::default(),
            )?,
        )
        .await?;
    Ok((case, record))
}
async fn counts(h: &Harness) -> Result<(i64, i64, i64)> {
    Ok((
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_acquisition_receipts")
            .fetch_one(&h.pool)
            .await?,
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_inbox")
            .fetch_one(&h.pool)
            .await?,
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox")
            .fetch_one(&h.pool)
            .await?,
    ))
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn ready_and_rejection_commit_once_and_terminal_recovery_skips_work() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let (case, r) = admit(&h, 1).await?;
        let access = Access::new(h.now)?;
        let objects = Storage::default();
        let c = Cancellation::default();
        let token = case
            .catalogs()
            .admit(&case.request, case.request.admitted_at, &c)?;
        let proposal = prepare(
            &r,
            &token,
            case.catalogs(),
            &RetainedEvidenceStorage::new(
                &objects,
                &PostgresAcquisitionLedger::new(h.pool.clone()),
            ),
            &access,
            None,
            100,
        )
        .await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let d = delivery(r.command(), "command")?;
        assert!(ledger.recover(&d).await?.is_none());
        assert_eq!(counts(&h).await?, (0, 0, 0));
        access.denied.store(true, Ordering::SeqCst);
        kind(
            ledger.commit(&d, &proposal, &access).await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(counts(&h).await?, (0, 0, 0));
        access.denied.store(false, Ordering::SeqCst);
        let receipt = ledger.commit(&d, &proposal, &access).await?;
        assert_eq!(counts(&h).await?, (1, 1, 1));
        let pool = h.reconnect().await?;
        let restarted = PostgresAcquisitionLedger::new(pool.clone());
        access.denied.store(true, Ordering::SeqCst); // Historical receipt is not an object-read grant.
        let recovered = finish_acquisition(&restarted, &d, &access, || async {
            Err(BudgetError::new(BudgetErrorKind::Invalid))
        })
        .await?;
        assert_eq!(receipt.bytes(), recovered.bytes());
        let new_delivery = delivery(r.command(), "logical-retry")?;
        assert_eq!(
            restarted
                .recover(&new_delivery)
                .await?
                .ok_or("receipt")?
                .bytes(),
            receipt.bytes()
        );
        assert_eq!(counts(&h).await?, (1, 2, 1));
        kind(
            ledger.begin_attempt(r.command(), 1).await,
            BudgetErrorKind::Terminal,
        )?;
        kind(
            ledger.start_call(r.command(), None).await,
            BudgetErrorKind::Terminal,
        )?;
        let (_, rejected) = admit(&h, 2).await?;
        ledger.begin_attempt(rejected.command(), 1).await?;
        h.configure_credits().await?;
        // Seed another account-rate fixture only; no existing external call is released.
        let permit = ledger.start_call(rejected.command(), None).await?;
        ledger.cancel(rejected.command()).await?;
        ledger
            .commit(
                &delivery(rejected.command(), "cancelled")?,
                &TerminalProposal::rejected(
                    rejected.command().clone(),
                    EvidenceRejection::Cancelled,
                ),
                &access,
            )
            .await?;
        assert!(ledger.load(rejected.command()).await?.1.pending.is_some());
        ledger
            .complete_call(&permit, CallCompletion::QuiescedUnknown)
            .await?;
        assert_eq!(
            ledger.load(rejected.command()).await?.1.charged_bytes,
            1_048_576
        );
        kind(
            ledger.start_call(rejected.command(), None).await,
            BudgetErrorKind::Terminal,
        )?;
        pool.close().await;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn receipt_write_failure_rolls_back_inbox_outbox_heads_and_observations() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let (case,r)=admit(&h,1).await?;let access=Access::new(h.now)?;let objects=Storage::default();let c=Cancellation::default();
        let proposal=prepare(&r,&case.catalogs().admit(&case.request,case.request.admitted_at,&c)?,case.catalogs(),&RetainedEvidenceStorage::new(&objects,&PostgresAcquisitionLedger::new(h.pool.clone())),&access,None,100).await?;
        sqlx::raw_sql("CREATE FUNCTION reject_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic receipt rollback'; END $$;
            CREATE TRIGGER reject_receipt BEFORE INSERT ON edgeagent_acquisition_receipts FOR EACH ROW EXECUTE FUNCTION reject_receipt();")
            .execute(&h.pool).await?;
        let ledger=PostgresAcquisitionLedger::new(h.pool.clone());let d=delivery(r.command(),"first")?;
        kind(ledger.commit(&d,&proposal,&access).await,BudgetErrorKind::Storage)?;
        assert_eq!(counts(&h).await?,(0,0,0));
        let heads:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_evidence_heads").fetch_one(&h.pool).await?;
        let versions:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_observation_versions").fetch_one(&h.pool).await?;
        assert_eq!((heads,versions),(0,0));assert_eq!(objects.calls.load(Ordering::SeqCst),1);
        sqlx::raw_sql("DROP TRIGGER reject_receipt ON edgeagent_acquisition_receipts").execute(&h.pool).await?;
        ledger.commit(&d,&proposal,&access).await?;assert_eq!(counts(&h).await?,(1,1,1));Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn competing_terminal_outcomes_and_changed_inbox_identity_fail_closed() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let (_, r) = admit(&h, 1).await?;
        let access = Access::new(h.now)?;
        let a = PostgresAcquisitionLedger::new(h.pool.clone());
        let pool = h.reconnect().await?;
        let b = PostgresAcquisitionLedger::new(pool.clone());
        let d = delivery(r.command(), "same")?;
        let p = TerminalProposal::rejected(r.command().clone(), EvidenceRejection::Provider);
        let q = TerminalProposal::rejected(r.command().clone(), EvidenceRejection::Budget);
        let (one, two) = tokio::join!(a.commit(&d, &p, &access), b.commit(&d, &q, &access));
        let winner = match (one, two) {
            (Ok(r), Err(e)) | (Err(e), Ok(r)) => {
                assert_eq!(e.kind(), BudgetErrorKind::Conflict);
                r
            }
            _ => return Err("expected one terminal winner".into()),
        };
        assert_eq!(counts(&h).await?, (1, 1, 1));
        assert_eq!(
            a.recover(&d).await?.ok_or("receipt")?.bytes(),
            winner.bytes()
        );
        let mut changed: serde_json::Value = serde_json::from_slice(&d.to_json()?)?;
        changed["correlationid"] = "changed-correlation".into();
        let changed =
            edgeagent_contracts::MessageEnvelope::from_json(&serde_json::to_vec(&changed)?)?;
        kind(a.recover(&changed).await, BudgetErrorKind::Conflict)?;
        assert_eq!(counts(&h).await?, (1, 1, 1));
        pool.close().await;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn concurrent_corrections_fence_heads_and_changed_observation_versions() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let access = Access::new(h.now)?;
        let objects = Storage::default();
        let c = Cancellation::default();
        let (case, r) = admit(&h, 1).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let p = prepare(
            &r,
            &case
                .catalogs()
                .admit(&case.request, case.request.admitted_at, &c)?,
            case.catalogs(),
            &RetainedEvidenceStorage::new(
                &objects,
                &PostgresAcquisitionLedger::new(h.pool.clone()),
            ),
            &access,
            None,
            100,
        )
        .await?;
        ledger
            .commit(&delivery(r.command(), "original")?, &p, &access)
            .await?;
        let previous = objects
            .objects
            .lock()
            .map_err(|_| "objects")?
            .last()
            .cloned()
            .ok_or("original")?;
        let (left, l) = admit(&h, 2).await?;
        let (right, r) = admit(&h, 3).await?;
        let p = prepare(
            &l,
            &left
                .catalogs()
                .admit(&left.request, left.request.admitted_at, &c)?,
            left.catalogs(),
            &RetainedEvidenceStorage::new(
                &objects,
                &PostgresAcquisitionLedger::new(h.pool.clone()),
            ),
            &access,
            Some(&previous),
            100,
        )
        .await?;
        let q = prepare(
            &r,
            &right
                .catalogs()
                .admit(&right.request, right.request.admitted_at, &c)?,
            right.catalogs(),
            &RetainedEvidenceStorage::new(
                &objects,
                &PostgresAcquisitionLedger::new(h.pool.clone()),
            ),
            &access,
            Some(&previous),
            100,
        )
        .await?;
        let ld = delivery(l.command(), "left")?;
        let rd = delivery(r.command(), "right")?;
        let (a, b) = tokio::join!(
            ledger.commit(&ld, &p, &access),
            ledger.commit(&rd, &q, &access)
        );
        let winner = match (a, b) {
            (Ok(r), Err(e)) | (Err(e), Ok(r)) => {
                assert_eq!(e.kind(), BudgetErrorKind::Conflict);
                r
            }
            _ => return Err("expected one correction winner".into()),
        };
        let EvidenceOutcome::Ready { object } = &winner.decision().data().outcome else {
            return Err("ready winner".into());
        };
        let current = objects
            .objects
            .lock()
            .map_err(|_| "objects")?
            .iter()
            .find(|a| a.digest() == &object.sha256)
            .cloned()
            .ok_or("winner artifact")?;
        let (case, r) = admit(&h, 4).await?;
        let changed = prepare(
            &r,
            &case
                .catalogs()
                .admit(&case.request, case.request.admitted_at, &c)?,
            case.catalogs(),
            &RetainedEvidenceStorage::new(
                &objects,
                &PostgresAcquisitionLedger::new(h.pool.clone()),
            ),
            &access,
            Some(&current),
            101,
        )
        .await?;
        kind(
            ledger
                .commit(
                    &delivery(r.command(), "changed-observation")?,
                    &changed,
                    &access,
                )
                .await,
            BudgetErrorKind::Conflict,
        )?;
        assert_eq!(counts(&h).await?, (2, 2, 2));
        let versions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_observation_versions")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(versions, 1);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
