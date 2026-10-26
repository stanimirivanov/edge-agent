//! Real independently authenticated roles and transaction boundaries; no vendor I/O.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned SQL role/schema administration and fault injection only."
)]
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/publication.rs"]
mod fixtures;
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger as Ledger;
use futures_util::FutureExt;
use std::{num::NonZeroU64, panic::AssertUnwindSafe};
use support::*;
async fn claimed(
    ledger: Ledger,
) -> Result<(Ledger, edgeagent_market_data_postgres::QualificationWorker)> {
    let claim = ledger.claim_qualification_worker().await?;
    Ok((ledger.bind_qualification_worker(&claim)?, claim))
}

fn time(ms: i64) -> Result<EvidenceTime> {
    Ok(EvidenceTime {
        seconds: ms.div_euclid(1000),
        nanos: u32::try_from(ms.rem_euclid(1000))? * 1_000_000,
    })
}
async fn setup_with_calls(
    h: &Harness,
    id: u128,
    calls: u32,
) -> Result<(
    AdmissionRecord,
    QualificationProfile,
    [QualificationControlReview; 5],
)> {
    let mut case = Case::with_precision(h.now, id, TimestampPrecision::Nanoseconds)?;
    case.request.budget.calls = std::num::NonZeroU32::new(calls).ok_or("calls")?;
    let record = case
        .record()?
        .with_configuration(&DailyEvidenceArtifact::hash(
            b"invented-reviewed-configuration",
        ))?;
    let owner = Ledger::new(h.pool.clone());
    owner
        .admit(
            &record,
            &case.catalogs().admit(
                &case.request,
                case.request.admitted_at,
                &Cancellation::default(),
            )?,
        )
        .await?;
    h.configure_credits().await?;
    let credits = DailyCreditPolicy::new(1, 100, h.now + 86_400_000, &[(ReferenceId::new(1)?, 1)])?;
    let version = |id: u128| -> Result<EvidenceVersion> {
        Ok(EvidenceVersion {
            id: EvidenceId::new(&format!("{id:032x}"))?,
            revision: EvidenceRevision::new("1")?,
        })
    };
    let profile = QualificationProfile::new(QualificationProfileData {
        version: QualificationProfileFormat::V1,
        provider: QualificationProvider::AlpacaHistoricalSipDaily,
        admission: record.command().clone(),
        configuration_sha256: EvidenceDigest::new(record.configuration().ok_or("configuration")?)?,
        owner_review: version(900)?,
        credits,
        not_before: time(h.now)?,
        expires_at: time(
            record
                .valid_until_ms()?
                .min((h.now.div_euclid(86_400_000) + 1) * 86_400_000),
        )?,
    })?;
    use QualificationControl::*;
    let controls = [
        DataOnlyCredentialScope,
        ReferenceProvenance,
        RuntimeFencing,
        RetentionAndCleanup,
        RestoreAndConsumers,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, control)| -> Result<_> {
        Ok(QualificationControlReview {
            control,
            evidence: version(100 + u128::try_from(i)?)?,
            known_at: UtcInstant::from_unix_millis(h.now)?,
            expires_at: UtcInstant::from_unix_millis(record.delete_by_ms()?)?,
        })
    })
    .collect::<Result<Vec<_>>>()?
    .try_into()
    .map_err(|_| "controls")?;
    Ok((record, profile, controls))
}
async fn setup(
    h: &Harness,
    id: u128,
) -> Result<(
    AdmissionRecord,
    QualificationProfile,
    [QualificationControlReview; 5],
)> {
    setup_with_calls(h, id, 12).await
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
async fn qualification_roles_require_independent_owner_and_exact_current_profile() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let (record, profile, controls) = setup(&h, 1).await?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::for_qualification(pool.clone(), profile.clone());
        kind(
            worker.begin_attempt(record.command(), 1).await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            worker.approve_qualification(&profile, &controls).await,
            BudgetErrorKind::Denied,
        )?;
        let sql = sqlx::query("UPDATE edgeagent_qualification_grants SET approved=TRUE")
            .execute(&pool)
            .await
            .err()
            .ok_or("worker wrote owner state")?;
        assert_eq!(
            sql.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        let mut invalid_controls = controls.clone();
        invalid_controls[0].expires_at = UtcInstant::from_unix_millis(record.delete_by_ms()? - 1)?;
        kind(
            owner
                .approve_qualification(&profile, &invalid_controls)
                .await,
            BudgetErrorKind::Denied,
        )?;
        owner.approve_qualification(&profile, &controls).await?;
        owner.approve_qualification(&profile, &controls).await?;
        kind(worker.install_qualification_input(record.command(),b"{}").await,BudgetErrorKind::Denied)?;
        owner.install_qualification_input(record.command(),b"{}").await?;
        assert_eq!(worker.qualification_input().await?,b"{}");
        kind(owner.install_qualification_input(record.command(),b"changed").await,BudgetErrorKind::Conflict)?;
        let snapshot=worker.qualification_snapshot().await?;
        assert!(snapshot.assess(&profile,&record,snapshot.at())?.is_some());
        assert!(snapshot.assess(&profile,&record,UtcInstant::from_unix_millis(snapshot.at().unix_millis()+1)?).is_err());
        let first=worker.claim_qualification_worker().await?;
        let stale=worker.bind_qualification_worker(&first)?;
        kind(worker.claim_qualification_worker().await,BudgetErrorKind::Busy)?;
        // A dropped future/handle never releases a durable claim. Only the owner
        // can assert physical quiescence, and stale generations cannot clear successors.
        let account=EvidenceId::new(&format!("{:032x}",record.request().scope.account.get()))?;
        kind(worker.confirm_qualification_worker_quiescent(&account,1).await,BudgetErrorKind::Denied)?;
        kind(owner.confirm_qualification_worker_quiescent(&account,2).await,BudgetErrorKind::Conflict)?;
        owner.confirm_qualification_worker_quiescent(&account,1).await?;
        let second=worker.claim_qualification_worker().await?;
        kind(stale.start_call(record.command(),None).await,BudgetErrorKind::Denied)?;
        kind(worker.check_qualification_worker(&first).await,BudgetErrorKind::Denied)?;
        worker.check_qualification_worker(&second).await?;
        kind(owner.confirm_qualification_worker_quiescent(&account,1).await,BudgetErrorKind::Conflict)?;
        owner.confirm_qualification_worker_quiescent(&account,2).await?;
        // Early-bound input read cannot be replaced with worker-authored temp data.
        sqlx::raw_sql("CREATE TEMP TABLE edgeagent_qualification_inputs(request_id TEXT,document BYTEA); INSERT INTO edgeagent_qualification_inputs VALUES('invented','\\x66616b65')").execute(&pool).await?;
        assert_eq!(worker.qualification_input().await?,b"{}");
        // A connection-local temp table grants its creator INSERT/UPDATE, but
        // cannot become the independent owner's early-bound control relation.
        sqlx::raw_sql("CREATE TEMP TABLE edgeagent_qualification_grants(request_id TEXT, profile BYTEA, reviews BYTEA, approved BOOLEAN)").execute(&pool).await?;
        kind(worker.approve_qualification(&profile,&controls).await,BudgetErrorKind::Denied)?;
        // NOINHERIT membership still permits SET ROLE and must be treated as owner-capable.
        let reviewer = format!("{role}_reviewer");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE {reviewer} NOLOGIN;
            GRANT UPDATE ON edgeagent_qualification_grants TO {reviewer};
            GRANT {reviewer} TO {role};"
        )))
        .execute(&h.pool)
        .await?;
        kind(
            worker.begin_attempt(record.command(), 1).await,
            BudgetErrorKind::Denied,
        )?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "REVOKE {reviewer} FROM {role}; DROP OWNED BY {reviewer}; DROP ROLE {reviewer};"
        )))
        .execute(&h.pool)
        .await?;
        kind(
            owner.begin_attempt(record.command(), 1).await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            Ledger::new(pool.clone())
                .begin_attempt(record.command(), 1)
                .await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            Ledger::for_qualification(h.pool.clone(), profile.clone())
                .begin_attempt(record.command(), 1)
                .await,
            BudgetErrorKind::Denied,
        )?;
        let mut changed = profile.data().clone();
        changed.owner_review.revision = EvidenceRevision::new("2")?;
        kind(
            Ledger::for_qualification(pool.clone(), QualificationProfile::new(changed)?)
                .begin_attempt(record.command(), 1)
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let (worker,_claim)=claimed(worker).await?;
        worker.begin_attempt(record.command(), 1).await?;
        let permit = worker.start_call(record.command(), None).await?;
        let before = worker.load(record.command()).await?.1;
        kind(
            worker
                .complete_call(&permit, CallCompletion::QuiescedUnknown)
                .await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(before, worker.load(record.command()).await?.1);
        owner.withdraw_qualification(&profile).await?;
        kind(
            worker.begin_attempt(record.command(), 2).await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            owner.approve_qualification(&profile, &controls).await,
            BudgetErrorKind::Denied,
        )?;
        owner
            .complete_call(&permit, CallCompletion::QuiescedUnknown)
            .await?;
        let after = worker.load(record.command()).await?.1;
        assert!(after.pending.is_none());
        assert_eq!(after.charged_bytes, before.charged_bytes);
        assert_eq!(after.calls, before.calls);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn qualification_withdrawal_and_terminal_failure_roll_back_all_effects() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async{
        let (record,profile,controls)=setup(&h,1).await?;
        let owner=Ledger::new(h.pool.clone());owner.approve_qualification(&profile,&controls).await?;
        let worker=Ledger::for_qualification(pool.clone(),profile.clone());
        let delivery=fixtures::delivery(record.command(),"qualification-withdrawn")?;
        let proposal=TerminalProposal::rejected(record.command().clone(),EvidenceRejection::Provider);
        let access=fixtures::Access::new(h.now)?;
        owner.withdraw_qualification(&profile).await?;
        kind(worker.commit(&delivery,&proposal,&access).await,BudgetErrorKind::Denied)?;
        assert_eq!(counts(&h).await?,(0,0,0));
        let (record,profile,controls)=setup(&h,2).await?;owner.approve_qualification(&profile,&controls).await?;
        let (worker,publication_claim)=claimed(Ledger::for_qualification(pool.clone(),profile.clone())).await?;
        let delivery=fixtures::delivery(record.command(),"qualification-approved")?;
        let proposal=TerminalProposal::rejected(record.command().clone(),EvidenceRejection::Provider);
        sqlx::raw_sql("CREATE FUNCTION fail_qualification_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'invented rollback'; END $$;
            CREATE TRIGGER qualification_failure BEFORE INSERT ON edgeagent_acquisition_receipts FOR EACH ROW EXECUTE FUNCTION fail_qualification_receipt();").execute(&h.pool).await?;
        kind(worker.commit(&delivery,&proposal,&access).await,BudgetErrorKind::Storage)?;
        assert_eq!(counts(&h).await?,(0,0,0));
        sqlx::raw_sql("DROP TRIGGER qualification_failure ON edgeagent_acquisition_receipts").execute(&h.pool).await?;
        let receipt=worker.commit(&delivery,&proposal,&access).await?;
        owner.withdraw_qualification(&profile).await?;
        assert_eq!(worker.recover(&delivery).await?.ok_or("receipt")?.bytes(),receipt.bytes());
        assert_eq!(counts(&h).await?,(1,1,1));
        worker.finish_qualification_worker(publication_claim).await?;
        // Terminal/preparation recovery must not need another provider call.
        let (last,profile,controls)=setup_with_calls(&h,3,1).await?;
        owner.approve_qualification(&profile,&controls).await?;
        let (worker,source_claim)=claimed(Ledger::for_qualification(pool.clone(),profile)).await?;
        worker.begin_attempt(last.command(),1).await?;
        let call=worker.start_call(last.command(),None).await?;
        worker.complete_call(&call,CallCompletion::Observed{decoded_bytes:0,retry_after_ms:None}).await?;
        kind(worker.qualification_snapshot().await,BudgetErrorKind::Exhausted)?;
        let account=EvidenceId::new(&format!("{:032x}",last.request().scope.account.get()))?;
        owner.confirm_qualification_worker_quiescent(&account,source_claim.generation()).await?;
        let (worker,claim)=claimed(worker).await?;
        let event=fixtures::delivery(last.command(),"last-call-publication")?;
        worker.commit(&event,&TerminalProposal::rejected(last.command().clone(),EvidenceRejection::Provider),&access).await?;
        worker.finish_qualification_worker(claim).await?;
        kind(worker.claim_qualification_worker().await,BudgetErrorKind::Terminal)?;
        let (cancelled,profile,controls)=setup(&h,4).await?;
        owner.approve_qualification(&profile,&controls).await?;
        let worker=Ledger::for_qualification(pool.clone(),profile);
        worker.cancel(cancelled.command()).await?;
        kind(worker.claim_qualification_worker().await,BudgetErrorKind::Cancelled)?;
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn qualification_retirement_erases_grants_without_owner_reconciliation_bypass() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let (record, profile, controls) = setup(&h, 1).await?;
        let owner = Ledger::new(h.pool.clone());
        owner.approve_qualification(&profile, &controls).await?;
        let worker = Ledger::for_qualification(pool.clone(), profile.clone());
        let (worker, _claim) = claimed(worker).await?;
        worker.begin_attempt(record.command(), 1).await?;
        let _pending = worker.start_call(record.command(), None).await?;
        owner.withdraw_qualification(&profile).await?;
        // Retirement by the trusted owner, not an elapsed lease or a worker assertion.
        owner
            .revoke_evidence_scope(&record.retention_scope()?)
            .await?;
        owner.retire_due_evidence(16).await?;
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_qualification_grants")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(remaining, 0);
        let item = owner
            .pending_cleanup(16)
            .await?
            .into_iter()
            .find(|i| i.request == record.command().request)
            .ok_or("retired inventory")?;
        let evidence = EvidenceVersion {
            id: EvidenceId::new(&format!("{:032x}", 901))?,
            revision: EvidenceRevision::new("1")?,
        };
        kind(
            worker.approve_qualification_cleanup(&item, &evidence).await,
            BudgetErrorKind::Denied,
        )?;
        kind(
            owner.qualification_cleanup(&item.request).await,
            BudgetErrorKind::Denied,
        )?;
        owner
            .approve_qualification_cleanup(&item, &evidence)
            .await?;
        let authority = owner.qualification_cleanup(&item.request).await?;
        authority.confirm_writes_quiescent(&item.request)?;
        assert!(
            authority
                .confirm_writes_quiescent(&EvidenceId::new(&format!("{:032x}", 902))?)
                .is_err()
        );
        owner.complete_cleanup(&item.request).await?;
        let private: i64 =
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_qualification_cleanup")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(private, 0);
        let account = EvidenceId::new(&format!("{:032x}", record.request().scope.account.get()))?;
        let generation = owner.account_block(&account).await?.ok_or("block")?;
        assert!(NonZeroU64::new(u64::try_from(generation)?).is_some());
        let fake = ClaimedQuiescence;
        kind(
            worker
                .confirm_retired_account(&account, generation, &fake)
                .await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(owner.account_block(&account).await?, Some(generation));
        owner
            .confirm_retired_account(&account, generation, &fake)
            .await?;
        assert_eq!(owner.account_block(&account).await?, None);
        kind(
            worker.begin_attempt(record.command(), 2).await,
            BudgetErrorKind::Denied,
        )?;
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn qualification_withdrawal_races_and_policy_mismatches_leave_atomic_account_state() -> Result
{
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let (record, profile, controls) = setup(&h, 1).await?;
        let owner = Ledger::new(h.pool.clone());
        owner.approve_qualification(&profile, &controls).await?;
        let worker = Ledger::for_qualification(pool.clone(), profile.clone());
        let (worker, _claim) = claimed(worker).await?;
        worker.begin_attempt(record.command(), 1).await?;
        let (started, withdrawn) = tokio::join!(
            worker.start_call(record.command(), None),
            owner.withdraw_qualification(&profile)
        );
        withdrawn?;
        let state = worker.load(record.command()).await?.1;
        let credits = owner
            .daily_credit_usage(record.request().scope.account)
            .await?
            .ok_or("credits")?;
        match started {
            Ok(permit) => {
                assert_eq!(state.calls, 1);
                assert_eq!(state.pending, Some(permit.clone()));
                assert_eq!(credits.charged, 1);
                owner
                    .complete_call(&permit, CallCompletion::QuiescedUnknown)
                    .await?;
            }
            Err(e) => {
                assert_eq!(e.kind(), BudgetErrorKind::Denied);
                assert_eq!(state.calls, 0);
                assert!(state.pending.is_none());
                assert_eq!(credits.charged, 0);
            }
        }
        let (record, profile, controls) = setup(&h, 2).await?;
        let mut data = profile.data().clone();
        data.credits =
            DailyCreditPolicy::new(1, 100, h.now + 86_400_000, &[(ReferenceId::new(1)?, 2)])?;
        let profile = QualificationProfile::new(data)?;
        owner.approve_qualification(&profile, &controls).await?;
        let worker = Ledger::for_qualification(pool.clone(), profile);
        let before = worker.load(record.command()).await?.1;
        kind(
            worker.begin_attempt(record.command(), 1).await,
            BudgetErrorKind::Conflict,
        )?;
        assert_eq!(worker.load(record.command()).await?.1, before);
        // Removing approval privileges must not launder a prior approving LOGIN
        // into an independent worker for the grant it authored itself.
        let (record, profile, controls) = setup(&h, 3).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "GRANT INSERT,UPDATE ON edgeagent_qualification_grants TO {role}"
        )))
        .execute(&h.pool)
        .await?;
        Ledger::new(pool.clone())
            .approve_qualification(&profile, &controls)
            .await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "REVOKE INSERT,UPDATE ON edgeagent_qualification_grants FROM {role}"
        )))
        .execute(&h.pool)
        .await?;
        let independent = Ledger::for_qualification(pool.clone(), profile);
        kind(
            independent.begin_attempt(record.command(), 1).await,
            BudgetErrorKind::Denied,
        )?;
        assert_eq!(independent.load(record.command()).await?.1.attempts, 0);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

struct ClaimedQuiescence;
impl CleanupAuthority for ClaimedQuiescence {
    fn authorize_deletion(
        &self,
        _: &EvidenceId,
        _: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        Ok(())
    }
    fn confirm_writes_quiescent(&self, _: &EvidenceId) -> std::result::Result<(), BudgetError> {
        Ok(())
    }
    fn confirm_account_quiescent(
        &self,
        _: &EvidenceId,
        _: i64,
    ) -> std::result::Result<(), BudgetError> {
        Ok(())
    }
}
