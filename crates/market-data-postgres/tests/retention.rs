//! Isolated owner-private retirement transactions; no provider calls or licensed fixtures.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned schema setup and deterministic fault/clock injection."
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
use std::panic::AssertUnwindSafe;
use support::*;

struct Owner(bool);

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn retiring_old_snapshots_preserves_new_heads_and_shared_version_ownership() -> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let (c, r) = admit(&h, 1).await?;
        let (c2, r2) = admit(&h, 2).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let access = Access::new(h.now)?;
        let raw = Storage::default();
        let cancel = Cancellation::default();
        let managed = RetainedEvidenceStorage::new(&raw, &ledger);
        let t = c
            .catalogs()
            .admit(&c.request, c.request.admitted_at, &cancel)?;
        let p = prepare(&r, &t, c.catalogs(), &managed, &access, None, 100).await?;
        ledger
            .commit(&delivery(r.command(), "original")?, &p, &access)
            .await?;
        let previous = raw
            .objects
            .lock()
            .map_err(|_| "poison")?
            .first()
            .cloned()
            .ok_or("original")?;
        ledger.stage_precise_preparation(&previous).await?;
        assert_eq!(
            ledger
                .recover_precise_preparation(r.command(), &access)
                .await?
                .ok_or("precise preparation")?,
            previous
        );
        let reconnected = PostgresAcquisitionLedger::new(h.reconnect().await?);
        assert_eq!(
            reconnected
                .recover_precise_preparation(r.command(), &access)
                .await?
                .ok_or("restarted preparation")?,
            previous
        );
        let mut changed = previous.data().clone();
        changed.bars.first_mut().ok_or("bar")?.volume.coefficient = "1".into();
        kind(
            ledger
                .stage_precise_preparation(&DailyEvidenceArtifact::new(changed)?)
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let t2 = c2
            .catalogs()
            .admit(&c2.request, c2.request.admitted_at, &cancel)?;
        let p2 = prepare(
            &r2,
            &t2,
            c2.catalogs(),
            &managed,
            &access,
            Some(&previous),
            100,
        )
        .await?;
        ledger
            .commit(&delivery(r2.command(), "correction")?, &p2, &access)
            .await?;
        sqlx::query("UPDATE edgeagent_acquisitions SET delete_ms=0 WHERE request_id=$1")
            .bind(r.command().request.as_str())
            .execute(&h.pool)
            .await?;
        // Exclusive retention denies immediately, even before the sweep runs.
        kind(
            ledger.recover(&delivery(r.command(), "expired")?).await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(ledger.retire_due_evidence(16).await?, 1);
        let head: String = sqlx::query_scalar("SELECT digest FROM edgeagent_evidence_heads")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(head, p2.head().ok_or("head")?.digest.as_str());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_observation_versions")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 1);
        let EvidenceOutcome::Ready { object } = &p2.decision().data().outcome else {
            return Err("ready".into());
        };
        ledger.check(object).await?;
        sqlx::query("UPDATE edgeagent_acquisitions SET delete_ms=0")
            .execute(&h.pool)
            .await?;
        assert_eq!(ledger.retire_due_evidence(16).await?, 1);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_observation_versions")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 0);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
impl CleanupAuthority for Owner {
    fn authorize_deletion(
        &self,
        request: &EvidenceId,
        _: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        self.confirm_writes_quiescent(request)
    }
    fn confirm_writes_quiescent(&self, _: &EvidenceId) -> std::result::Result<(), BudgetError> {
        if self.0 {
            Ok(())
        } else {
            Err(BudgetError::new(BudgetErrorKind::Denied))
        }
    }
    fn confirm_account_quiescent(
        &self,
        id: &EvidenceId,
        _: i64,
    ) -> std::result::Result<(), BudgetError> {
        self.confirm_writes_quiescent(id)
    }
}
async fn admit(h: &Harness, id: u128) -> Result<(Case, AdmissionRecord)> {
    let c = Case::new(h.now, id)?;
    let r = c.record()?;
    PostgresAcquisitionLedger::new(h.pool.clone())
        .admit(
            &r,
            &c.catalogs()
                .admit(&c.request, c.request.admitted_at, &Cancellation::default())?,
        )
        .await?;
    Ok((c, r))
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn retiring_uncertain_calls_erases_fingerprints_but_preserves_fenced_account_blocks() -> Result
{
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let (c, r) = admit(&h, 1).await?;
        let (_, other) = admit(&h, 2).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        ledger.begin_attempt(r.command(), 1).await?;
        h.configure_credits().await?;
        let permit = ledger.start_call(r.command(), None).await?;
        // Age only the owner-private deadline fixture, not the machine clock or provider.
        sqlx::query("UPDATE edgeagent_acquisitions SET orphan_ms=0 WHERE request_id=$1")
            .bind(r.command().request.as_str())
            .execute(&h.pool)
            .await?;
        assert_eq!(ledger.retire_due_evidence(1).await?, 1);
        kind(ledger.load(r.command()).await, BudgetErrorKind::Denied)?;
        kind(
            ledger
                .complete_call(&permit, CallCompletion::QuiescedUnknown)
                .await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            ledger.begin_attempt(other.command(), 1).await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            ledger
                .admit(
                    &r,
                    &c.catalogs().admit(
                        &c.request,
                        c.request.admitted_at,
                        &Cancellation::default(),
                    )?,
                )
                .await,
            BudgetErrorKind::Denied,
        )?;
        let account = EvidenceId::new(&format!("{:032x}", c.request.scope.account.get()))?;
        let generation = ledger
            .account_block(&account)
            .await?
            .ok_or("missing block")?;
        let bytes: Vec<u8> = sqlx::query_scalar(
            "SELECT budget FROM edgeagent_acquisition_accounts WHERE account_id=$1",
        )
        .bind(account.as_str())
        .fetch_one(&h.pool)
        .await?;
        assert!(!String::from_utf8(bytes.clone())?.contains(r.command().request_sha256.as_str()));
        assert!(AccountBudget::decode(&bytes).is_ok());
        kind(
            ledger
                .confirm_retired_account(&account, generation, &Owner(false))
                .await,
            BudgetErrorKind::Denied,
        )?;
        // Model another retirement generation; stale owner evidence cannot clear it.
        sqlx::query(
            "UPDATE edgeagent_acquisition_accounts SET block_generation=block_generation+1",
        )
        .execute(&h.pool)
        .await?;
        kind(
            ledger
                .confirm_retired_account(&account, generation, &Owner(true))
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let restarted = PostgresAcquisitionLedger::new(h.reconnect().await?);
        assert_eq!(
            restarted.account_block(&account).await?,
            Some(generation + 1)
        );
        let unchanged: Vec<u8> = sqlx::query_scalar(
            "SELECT budget FROM edgeagent_acquisition_accounts WHERE account_id=$1",
        )
        .bind(account.as_str())
        .fetch_one(&h.pool)
        .await?;
        assert_eq!(
            unchanged, bytes,
            "stale generation must also roll back credit changes"
        );
        restarted
            .confirm_retired_account(&account, generation + 1, &Owner(true))
            .await?;
        assert_eq!(restarted.account_block(&account).await?, None);
        // Unblocking preserves rate history, but conservatively closes today's credits
        // because the erased endpoint no longer supplies a trustworthy carry-forward price.
        let after: Vec<u8> = sqlx::query_scalar(
            "SELECT budget FROM edgeagent_acquisition_accounts WHERE account_id=$1",
        )
        .bind(account.as_str())
        .fetch_one(&h.pool)
        .await?;
        let after_state = AccountBudget::decode(&after)?;
        assert_eq!(after_state.daily_usage().ok_or("daily usage")?.charged, 100);
        let mut before_json: serde_json::Value = serde_json::from_slice(&bytes)?;
        let mut after_json: serde_json::Value = serde_json::from_slice(&after)?;
        before_json
            .as_object_mut()
            .ok_or("account")?
            .remove("daily");
        after_json.as_object_mut().ok_or("account")?.remove("daily");
        assert_eq!(before_json, after_json);
        // Idempotent confirmation cannot close a later day's allocation a second time.
        restarted
            .confirm_retired_account(&account, generation + 1, &Owner(true))
            .await?;
        let repeated: Vec<u8> = sqlx::query_scalar(
            "SELECT budget FROM edgeagent_acquisition_accounts WHERE account_id=$1",
        )
        .bind(account.as_str())
        .fetch_one(&h.pool)
        .await?;
        assert_eq!(repeated, after);
        kind(restarted.load(r.command()).await, BudgetErrorKind::Denied)?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn ready_evidence_survives_orphan_sweep_then_revocation_erases_private_publication_state()
-> Result {
    let h = Harness::new().await?;
    let result = AssertUnwindSafe(async {
        let (c, r) = admit(&h, 1).await?;
        let (c2, r2) = admit(&h, 2).await?;
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        let access = Access::new(h.now)?;
        let objects = Storage::default();
        let cancel = Cancellation::default();
        let token = c
            .catalogs()
            .admit(&c.request, c.request.admitted_at, &cancel)?;
        let p = prepare(&r, &token, c.catalogs(), &objects, &access, None, 100).await?;
        let artifact = objects
            .objects
            .lock()
            .map_err(|_| "poison")?
            .first()
            .cloned()
            .ok_or("object")?;
        ledger.stage(&artifact).await?;
        let EvidenceOutcome::Ready { object } = p.decision().data().outcome.clone() else {
            return Err("ready".into());
        };
        let mut redirected = object.clone();
        redirected.audience = EvidenceId::new(&"3".repeat(32))?;
        let error =
            sqlx::query("UPDATE edgeagent_evidence_objects SET descriptor=$1 WHERE request_id=$2")
                .bind(serde_json::to_vec(&redirected)?)
                .bind(r.command().request.as_str())
                .execute(&h.pool)
                .await
                .err()
                .ok_or("expected inventory redirection rejection")?;
        assert!(
            matches!(error, sqlx::Error::Database(ref e) if e.code().as_deref()==Some("23514"))
        );
        ledger
            .commit(&delivery(r.command(), "ready")?, &p, &access)
            .await?;
        let token2 = c2
            .catalogs()
            .admit(&c2.request, c2.request.admitted_at, &cancel)?;
        prepare(&r2, &token2, c2.catalogs(), &objects, &access, None, 100).await?;
        let orphan = objects
            .objects
            .lock()
            .map_err(|_| "poison")?
            .last()
            .cloned()
            .ok_or("orphan")?;
        ledger.stage(&orphan).await?;
        sqlx::query("UPDATE edgeagent_acquisitions SET orphan_ms=0")
            .execute(&h.pool)
            .await?;
        assert_eq!(ledger.retire_due_evidence(16).await?, 1); // Never collect a live receipt as an orphan.
        ledger.check(&object).await?;
        assert_eq!(ledger.pending_cleanup(16).await?.len(), 1);
        ledger.revoke_evidence_scope(&r.retention_scope()?).await?;
        kind(ledger.check(&object).await, BudgetErrorKind::Denied)?; // Denial precedes physical cleanup.
        kind(
            ledger
                .recover(&delivery(r.command(), "after-revocation")?)
                .await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(ledger.retire_due_evidence(16).await?, 1);
        for table in [
            "edgeagent_acquisitions",
            "edgeagent_acquisition_receipts",
            "edgeagent_evidence_heads",
            "edgeagent_observation_versions",
            "edgeagent_evidence_ownership",
            "edgeagent_message_inbox",
            "edgeagent_message_outbox",
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&h.pool)
                    .await?;
            assert_eq!(count, 0, "{table}");
        }
        let first = ledger
            .pending_cleanup(1)
            .await?
            .first()
            .ok_or("first")?
            .request
            .clone();
        let next = ledger
            .pending_cleanup(1)
            .await?
            .first()
            .ok_or("next")?
            .request
            .clone();
        assert_ne!(first, next); // An unfenced request cannot pin the front of the queue.
        let pending = ledger.pending_cleanup(16).await?;
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|i| i.objects.len() == 1));
        assert!(
            format!("{pending:?}")
                .find(object.sha256.as_str())
                .is_none()
        );
        // No permissive maintenance worker: invoke confirmation only after external deletes.
        for i in pending {
            ledger.complete_cleanup(&i.request).await?;
        }
        assert!(ledger.pending_cleanup(16).await?.is_empty());
        kind(ledger.stage(&artifact).await, BudgetErrorKind::Denied)?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn failed_retirement_rolls_back_denial_and_account_redaction_atomically() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let (_,r)=admit(&h,1).await?;
        let ledger=PostgresAcquisitionLedger::new(h.pool.clone());
        ledger.begin_attempt(r.command(),1).await?; h.configure_credits().await?; let permit=ledger.start_call(r.command(),None).await?;
        sqlx::raw_sql("UPDATE edgeagent_acquisitions SET orphan_ms=0;
          CREATE FUNCTION deny_retire() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'invented retirement fault'; END $$;
          CREATE TRIGGER deny_retire BEFORE DELETE ON edgeagent_acquisitions FOR EACH ROW EXECUTE FUNCTION deny_retire();").execute(&h.pool).await?;
        kind(ledger.retire_due_evidence(1).await,BudgetErrorKind::Storage)?;
        assert_eq!(ledger.load(r.command()).await?.1.pending,Some(permit));
        assert!(ledger.pending_cleanup(16).await?.is_empty());
        let blocked:bool=sqlx::query_scalar("SELECT blocked FROM edgeagent_acquisition_accounts").fetch_one(&h.pool).await?;
        assert!(!blocked);
        sqlx::raw_sql("DROP TRIGGER deny_retire ON edgeagent_acquisitions").execute(&h.pool).await?;
        assert_eq!(ledger.retire_due_evidence(1).await?,1);
        kind(ledger.load(r.command()).await,BudgetErrorKind::Denied)?;
        assert_eq!(ledger.retire_due_evidence(1).await?,0);
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
