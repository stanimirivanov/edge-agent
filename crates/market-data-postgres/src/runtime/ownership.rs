use super::*;
/// One account's committed process claim. Dropping it never releases uncertainty.
pub struct QualificationWorker(WorkerBinding);
#[derive(Clone)]
pub(crate) struct WorkerBinding {
    account: String,
    request: EvidenceId,
    generation: i64,
}
impl std::fmt::Debug for QualificationWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QualificationWorker(..)")
    }
}
impl QualificationWorker {
    /// Exact opaque process generation for independently authorized owner reconciliation.
    /// The number alone grants no authority and proves no physical quiescence.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.0.generation
    }
}
impl Ledger {
    /// Bind an obtained process claim to this exact approved acquisition. Performs no I/O.
    /// Every protected mutation rechecks/locks its generation inside the SQL transaction.
    /// # Errors
    /// Rejects unqualified ledgers or a claim belonging to another acquisition.
    pub fn bind_qualification_worker(&self, worker: &QualificationWorker) -> Result<Self> {
        let p = self.qualification.as_ref().ok_or_else(denied)?;
        if p.data().admission.request != worker.0.request {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        let mut bound = self.clone();
        bound.worker = Some(worker.0.clone());
        Ok(bound)
    }
    pub(crate) async fn guard_qualification_worker(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        record: &AdmissionRecord,
    ) -> Result<()> {
        if self.qualification.is_none() {
            return Ok(());
        }
        let expected = self.worker.as_ref().ok_or_else(denied)?;
        if expected.request != record.command().request
            || expected.account != format!("{:032x}", record.request().scope.account.get())
        {
            return Err(denied());
        }
        let state=sqlx::query!("SELECT generation,request_id,active FROM edgeagent_qualification_workers WHERE account_id=$1 FOR SHARE",expected.account)
            .fetch_optional(&mut **tx).await.map_err(BudgetError::storage)?.ok_or_else(denied)?;
        if !state.active
            || state.generation != expected.generation
            || state.request_id != expected.request.as_str()
        {
            return Err(denied());
        }
        Ok(())
    }
    /// Record external owner confirmation that every provider/write operation has stopped.
    /// This does not clear ledger pending calls or retired account blocks, or refund credits.
    /// A timeout, missing process or lease expiry is not sufficient evidence.
    /// # Errors
    /// Rejects non-owner sessions and stale generations; confirmation asserts actual quiescence.
    pub async fn confirm_qualification_worker_quiescent(
        &self,
        account: &EvidenceId,
        generation: i64,
    ) -> Result<()> {
        if generation <= 0 {
            return Err(invalid(()));
        }
        let mut tx = self.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let changed=sqlx::query!("UPDATE edgeagent_qualification_workers SET active=FALSE WHERE account_id=$1 AND generation=$2",account.as_str(),generation)
            .execute(&mut *tx).await.map_err(BudgetError::storage)?;
        if changed.rows_affected() != 1 {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
    /// Commit exclusive account ownership before any provider or object operation.
    /// A crash/unknown result leaves active=true indefinitely; no timeout permits takeover.
    /// # Errors
    /// Rejects existing uncertainty, absent independent approval or conflicting work.
    pub async fn claim_qualification_worker(&self) -> Result<QualificationWorker> {
        let p = self.qualification.as_ref().ok_or_else(denied)?;
        let mut tx = self.transaction().await?;
        crate::retention::guard_request(&mut tx, &p.data().admission.request).await?;
        let state=sqlx::query!("SELECT a.account_id,a.budget,a.blocked FROM edgeagent_acquisition_accounts a JOIN edgeagent_acquisitions q ON q.account_id=a.account_id WHERE q.request_id=$1 FOR UPDATE OF a",p.data().admission.request.as_str())
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        if state.blocked {
            return Err(denied());
        }
        let account_budget = AccountBudget::decode(&state.budget)?;
        if account_budget.has_pending_call() {
            return Err(BudgetError::new(BudgetErrorKind::Busy));
        }
        if !account_budget.matches_daily_policy(&p.data().credits) {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        let (r, budget) = ledger::load(&mut tx, &p.data().admission).await?;
        if budget.snapshot().cancelled {
            return Err(BudgetError::new(BudgetErrorKind::Cancelled));
        }
        let terminal=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts WHERE request_id=$1) AS "present!""#,r.command().request.as_str())
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        if terminal {
            return Err(BudgetError::new(BudgetErrorKind::Terminal));
        }
        qualification::guard(&mut tx, Some(p), &r, None).await?;
        let account = format!("{:032x}", r.request().scope.account.get());
        let claim=sqlx::query!("INSERT INTO edgeagent_qualification_workers(account_id,generation,request_id,active) VALUES($1,1,$2,TRUE) ON CONFLICT(account_id) DO UPDATE SET generation=edgeagent_qualification_workers.generation+1,request_id=EXCLUDED.request_id,active=TRUE WHERE NOT edgeagent_qualification_workers.active RETURNING generation",account,r.command().request.as_str())
            .fetch_optional(&mut *tx).await.map_err(BudgetError::storage)?.ok_or_else(||BudgetError::new(BudgetErrorKind::Busy))?;
        tx.commit().await.map_err(BudgetError::storage)?;
        Ok(QualificationWorker(WorkerBinding {
            account,
            request: r.command().request.clone(),
            generation: claim.generation,
        }))
    }
    /// Recheck exact process generation before starting another bounded external operation.
    /// # Errors
    /// Denies replaced or released workers; this does not establish remote quiescence.
    pub async fn check_qualification_worker(&self, worker: &QualificationWorker) -> Result<()> {
        let worker = &worker.0;
        let mut tx = self.transaction().await?;
        let valid=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_qualification_workers WHERE account_id=$1 AND request_id=$2 AND generation=$3 AND active) AS "valid!""#,worker.account,worker.request.as_str(),worker.generation)
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        if !valid {
            return Err(denied());
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
    /// Release only after terminal commit and after joining all external futures.
    /// Never call on cancellation, panic, failed PUT or unknown provider completion.
    /// # Errors
    /// Denies absent receipt or a replaced generation; no automatic recovery/refund occurs.
    pub async fn finish_qualification_worker(&self, worker: QualificationWorker) -> Result<()> {
        let worker = worker.0;
        let mut tx = self.transaction().await?;
        let changed=sqlx::query!("UPDATE edgeagent_qualification_workers SET active=FALSE WHERE account_id=$1 AND request_id=$2 AND generation=$3 AND active AND EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts WHERE request_id=$2)",worker.account,worker.request.as_str(),worker.generation)
            .execute(&mut *tx).await.map_err(BudgetError::storage)?;
        if changed.rows_affected() != 1 {
            return Err(denied());
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
}
