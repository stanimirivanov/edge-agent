//! Real SQL transaction/restart/denial proofs with invented data and no provider calls.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned synthetic schemas and rollback fault injection."
)]
#[path = "native/inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/native_store.rs"]
mod model;
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/publication.rs"]
mod publication;
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use futures_util::FutureExt;
use inputs::NativeFixture;
use model::NativeStore;
use publication::{Access, delivery};
use std::{panic::AssertUnwindSafe, sync::atomic::Ordering};
use support::*;

async fn admit(h: &Harness, id: u128) -> Result<NativeFixture> {
    let f = NativeFixture::at(id, h.now).map_err(|_| "native fixture")?;
    let c = Cancellation::default();
    let token =
        f.f.catalogs()
            .admit(&f.f.request, f.f.request.admitted_at, &c)?;
    PostgresAcquisitionLedger::new(h.pool.clone())
        .admit(&f.record, &token)
        .await?;
    Ok(f)
}
async fn proposal(
    f: &NativeFixture,
    ledger: &PostgresAcquisitionLedger,
    objects: &NativeStore,
    access: &Access,
    previous: Option<&NativeDailyArtifact>,
) -> Result<TerminalProposal> {
    let c = Cancellation::default();
    let token = f.f.catalogs().admit(&f.f.request, access.now(), &c)?;
    let mut input = f.input();
    input.previous = previous;
    Ok(TerminalProposal::ready_native(
        &f.record,
        &token,
        f.f.catalogs(),
        &c,
        input,
        &RetainedEvidenceStorage::new(objects, ledger),
        access,
    )
    .await?)
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
async fn native_preparation_survives_restart_and_terminal_ack_loss_without_new_identity() -> Result
{
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let f = admit(&h, 1).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let objects = NativeStore::default();
        let access = Access::new(h.now)?;
        objects.fail.store(true, Ordering::SeqCst);
        assert!(
            proposal(&f, &ledger, &objects, &access, None)
                .await
                .is_err()
        );
        assert_eq!(counts(&h).await?, (0, 0, 0));
        let pool = h.reconnect().await?;
        let restarted = PostgresAcquisitionLedger::new(pool.clone());
        let retained = restarted
            .recover_native(f.record.command(), &access)
            .await?
            .ok_or("preparation")?;
        let later = Access::new(h.now + 1000)?;
        objects.fail.store(false, Ordering::SeqCst);
        let p = proposal(&f, &restarted, &objects, &later, None).await?;
        assert_eq!(p.head().ok_or("head")?.digest, *retained.digest());
        let mut changed = retained.data().clone();
        changed.lineage[0].transformation = EvidenceRevision::new("9")?;
        kind(
            restarted
                .stage_native(&NativeDailyArtifact::new(changed)?)
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let d = delivery(f.record.command(), "native-first")?;
        let receipt = restarted.commit(&d, &p, &later).await?; // Discard the ACK; reconnect/recover below.
        let recovered = finish_acquisition(&ledger, &d, &later, || async {
            Err(BudgetError::new(BudgetErrorKind::Invalid))
        })
        .await?;
        assert_eq!(receipt.bytes(), recovered.bytes());
        ledger
            .recover(&delivery(f.record.command(), "native-redelivery")?)
            .await?
            .ok_or("receipt")?;
        assert_eq!(counts(&h).await?, (1, 2, 1));
        assert_eq!(objects.calls.load(Ordering::SeqCst), 2);
        pool.close().await;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn native_terminal_rollback_and_concurrent_corrections_preserve_atomic_fences() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let objects = NativeStore::default();
        let access = Access::new(h.now)?;
        let f = admit(&h, 1).await?;
        let p = proposal(&f, &ledger, &objects, &access, None).await?;
        sqlx::raw_sql(
            "CREATE FUNCTION reject_native() RETURNS trigger LANGUAGE plpgsql AS $$
                BEGIN RAISE EXCEPTION 'synthetic native rollback'; END $$;
             CREATE TRIGGER reject_native BEFORE INSERT ON edgeagent_acquisition_receipts
                FOR EACH ROW EXECUTE FUNCTION reject_native();",
        )
        .execute(&h.pool)
        .await?;
        let d = delivery(f.record.command(), "native-original")?;
        kind(
            ledger.commit(&d, &p, &access).await,
            BudgetErrorKind::Storage,
        )?;
        assert_eq!(counts(&h).await?, (0, 0, 0));
        let guards: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM edgeagent_evidence_heads)
                  + (SELECT count(*) FROM edgeagent_observation_versions)",
        )
        .fetch_one(&h.pool)
        .await?;
        assert_eq!(guards, 0);
        sqlx::raw_sql("DROP TRIGGER reject_native ON edgeagent_acquisition_receipts")
            .execute(&h.pool)
            .await?;
        ledger.commit(&d, &p, &access).await?;
        let previous = ledger
            .recover_native(f.record.command(), &access)
            .await?
            .ok_or("previous")?;
        let left = admit(&h, 2).await?;
        let right = admit(&h, 3).await?;
        let p = proposal(&left, &ledger, &objects, &access, Some(&previous)).await?;
        let q = proposal(&right, &ledger, &objects, &access, Some(&previous)).await?;
        let ld = delivery(left.record.command(), "native-left")?;
        let rd = delivery(right.record.command(), "native-right")?;
        let (a, b) = tokio::join!(
            ledger.commit(&ld, &p, &access),
            ledger.commit(&rd, &q, &access)
        );
        let winner = match (a, b) {
            (Ok(r), Err(e)) | (Err(e), Ok(r)) => {
                assert_eq!(e.kind(), BudgetErrorKind::Conflict);
                r
            }
            _ => return Err("one correction winner required".into()),
        };
        assert_eq!(counts(&h).await?, (2, 2, 2));
        let current = ledger
            .recover_native(&winner.decision().data().request, &access)
            .await?
            .ok_or("winner")?;
        let mut changed =
            NativeFixture::with_start(4, h.now, 31).map_err(|_| "overlapping native fixture")?;
        let c = Cancellation::default();
        let token = changed
            .f
            .catalogs()
            .admit(&changed.f.request, access.now(), &c)?;
        ledger.admit(&changed.record, &token).await?;
        let mut capture = changed.capture.data().clone();
        // A different range has no head yet, but overlaps owner observation 31/version 1.
        capture.rows[0].ohlc = std::array::from_fn(|_| EvidenceDecimal {
            coefficient: "102".into(),
            scale: 2,
        });
        changed.capture = DateOnlyDailyArtifact::new(capture)?;
        assert!(
            proposal(&changed, &ledger, &objects, &access, Some(&current))
                .await
                .is_err()
        );
        // This conflict must be the immutable-version fence, not the unrelated series head.
        let conflicting = proposal(&changed, &ledger, &objects, &access, None).await?;
        assert_ne!(
            conflicting.head().ok_or("overlap head")?.key,
            p.head().ok_or("original head")?.key
        );
        let retained_versions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_observation_versions")
                .fetch_one(&h.pool)
                .await?;
        kind(
            ledger
                .commit(
                    &delivery(changed.record.command(), "native-conflict")?,
                    &conflicting,
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
        assert_eq!(versions, retained_versions);
        let heads: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_evidence_heads")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(heads, 1);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn native_revocation_denies_recovery_and_retirement_erases_preparations_not_cleanup_keys()
-> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let f = admit(&h, 1).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let objects = NativeStore::default();
        let access = Access::new(h.now)?;
        let orphan = admit(&h, 2).await?;
        objects.fail.store(true, Ordering::SeqCst);
        assert!(
            proposal(&orphan, &ledger, &objects, &access, None)
                .await
                .is_err()
        );
        let orphan_artifact = ledger
            .recover_native(orphan.record.command(), &access)
            .await?
            .ok_or("orphan preparation")?;
        // Advance only this owned persisted orphan timer, not the real clock or live receipt.
        sqlx::query("UPDATE edgeagent_acquisitions SET orphan_ms=0 WHERE request_id=$1")
            .bind(orphan.record.command().request.as_str())
            .execute(&h.pool)
            .await?;
        assert_eq!(ledger.retire_due_evidence(16).await?, 1);
        kind(
            ledger
                .recover_native(orphan.record.command(), &access)
                .await,
            BudgetErrorKind::Denied,
        )?;
        objects.fail.store(false, Ordering::SeqCst);
        let p = proposal(&f, &ledger, &objects, &access, None).await?;
        ledger
            .commit(&delivery(f.record.command(), "native-ready")?, &p, &access)
            .await?;
        let a = ledger
            .recover_native(f.record.command(), &access)
            .await?
            .ok_or("preparation")?;
        ledger
            .revoke_evidence_scope(&f.record.retention_scope()?)
            .await?;
        kind(
            ledger.recover_native(f.record.command(), &access).await,
            BudgetErrorKind::Denied,
        )?;
        kind(ledger.stage_native(&a).await, BudgetErrorKind::Denied)?;
        assert!(
            RetainedEvidenceStorage::new(&objects, &ledger)
                .get_native(&a.object()?, &access)
                .await
                .is_err()
        );
        assert_eq!(ledger.retire_due_evidence(16).await?, 1);
        let preparations: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_native_preparations")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(preparations, 0);
        assert_eq!(counts(&h).await?, (0, 0, 0));
        let pending = ledger.pending_cleanup(16).await?;
        assert_eq!(pending.len(), 2);
        let first_object = a.object()?;
        assert!(
            pending
                .iter()
                .any(|p| p.request == f.record.command().request
                    && p.objects.as_slice() == std::slice::from_ref(&first_object))
        );
        let mut actual = pending
            .iter()
            .flat_map(|p| p.objects.clone())
            .collect::<Vec<_>>();
        let mut expected = vec![a.object()?, orphan_artifact.object()?];
        actual.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        expected.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        assert_eq!(actual, expected);
        kind(
            ledger.recover_native(f.record.command(), &access).await,
            BudgetErrorKind::Denied,
        )?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
