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

fn time(ms: i64) -> Result<EvidenceTime> {
    Ok(EvidenceTime {
        seconds: ms.div_euclid(1000),
        nanos: u32::try_from(ms.rem_euclid(1000))? * 1_000_000,
    })
}
async fn setup(
    h: &Harness,
    id: u128,
) -> Result<(
    AdmissionRecord,
    QualificationProfile,
    [QualificationControlReview; 5],
)> {
    let case = Case::with_precision(h.now, id, TimestampPrecision::Nanoseconds)?;
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
        let worker=Ledger::for_qualification(pool.clone(),profile.clone());
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
