use edgeagent_contracts::evidence::{AcquireEvidence, EvidenceId};
use edgeagent_market_data_application::{
    AccountBudget, AcquisitionBudget, AcquisitionLedger, AdmissionRecord, AuthorizedAcquisition,
    BudgetError as Error, BudgetErrorKind as Kind, BudgetSnapshot, CallCompletion, CallPermit,
    DailyCreditPolicy, DailyCreditUsage, ProviderAccountId,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::fmt;

enum MutationAuthority {
    Ordinary,
    Qualification,
    OwnerConfirmation,
}

/// PostgreSQL ledger. Bind a service-private schema and explicit pool bounds at composition.
/// Mutations lock scope, account then command, use database time and commit before return.
#[derive(Clone)]
pub struct PostgresAcquisitionLedger {
    pool: PgPool,
    pub(super) qualification: Option<edgeagent_market_data_application::QualificationProfile>,
    pub(super) worker: Option<crate::runtime::WorkerBinding>,
}
impl fmt::Debug for PostgresAcquisitionLedger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresAcquisitionLedger")
            .finish_non_exhaustive()
    }
}
impl PostgresAcquisitionLedger {
    /// Bind an explicitly configured owner pool; performs no connection or migration.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self {
            pool,
            qualification: None,
            worker: None,
        }
    }
    /// Bind one exact independently owner-approved profile for assessment/recovery.
    /// Claim and bind a qualification worker before protected mutations; this constructor
    /// alone confers no execution capability. Performs no approval or I/O.
    /// Reservation and first publication reject privileged/self-approving worker roles.
    #[must_use]
    pub fn for_qualification(
        pool: PgPool,
        profile: edgeagent_market_data_application::QualificationProfile,
    ) -> Self {
        Self {
            pool,
            qualification: Some(profile),
            worker: None,
        }
    }
    pub(super) fn bind(&self, command: &AcquireEvidence) -> Result<(), Error> {
        if self
            .qualification
            .as_ref()
            .is_some_and(|p| &p.data().admission != command)
        {
            return Err(Error::new(Kind::Conflict));
        }
        Ok(())
    }
    /// Install the owner's reviewed exclusive allocation, never inferred provider defaults.
    /// The trusted composition must fence former workers/other consumers and confirm all
    /// current-day charges. Unknown shared consumption must not be installed as zero.
    /// Identical retries do not reset charges; changed reviews require a future migration.
    /// # Errors
    /// Rejects missing/blocked accounts, pending work, conflicts and invalid reviews.
    pub async fn configure_daily_credits(
        &self,
        account: ProviderAccountId,
        policy: DailyCreditPolicy,
        already_charged: u32,
    ) -> Result<DailyCreditUsage, Error> {
        let account = format!("{:032x}", account.get());
        let mut tx = self.transaction().await?;
        crate::qualification::require_owner(&mut tx).await?;
        let row = sqlx::query!(
            "SELECT budget,blocked FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            account
        ).fetch_optional(&mut *tx).await.map_err(Error::storage)?
            .ok_or_else(|| Error::new(Kind::NotFound))?;
        if row.blocked {
            return Err(Error::new(Kind::Denied));
        }
        let mut shared = AccountBudget::decode(&row.budget)?;
        let now = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        shared.configure_daily_credits(policy, already_charged, now)?;
        let usage = shared
            .daily_usage()
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        let state = shared.encode()?;
        sqlx::query!(
            "UPDATE edgeagent_acquisition_accounts SET budget=$2 WHERE account_id=$1",
            account,
            state
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(usage)
    }
    /// Read persisted account usage, including blocked/expired accounts, without resetting it.
    /// This owner-private projection is not permission to dispatch or release evidence.
    /// # Errors
    /// Rejects absent accounts, invalid private state and storage failures.
    pub async fn daily_credit_usage(
        &self,
        account: ProviderAccountId,
    ) -> Result<Option<DailyCreditUsage>, Error> {
        let account = format!("{:032x}", account.get());
        let mut tx = self.transaction().await?;
        let row = sqlx::query!(
            "SELECT budget,blocked FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            account
        ).fetch_optional(&mut *tx).await.map_err(Error::storage)?
            .ok_or_else(|| Error::new(Kind::NotFound))?;
        let usage = AccountBudget::decode(&row.budget)?.daily_usage();
        tx.commit().await.map_err(Error::storage)?;
        Ok(usage)
    }
    pub(super) async fn transaction(&self) -> Result<Transaction<'_, Postgres>, Error> {
        let mut tx = self.pool.begin().await.map_err(Error::storage)?;
        sqlx::query!(
            "SELECT set_config('statement_timeout', '5000', true) AS statement_timeout,
                    set_config('lock_timeout', '1000', true) AS lock_timeout"
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        Ok(tx)
    }
    async fn mutate<T>(
        &self,
        command: &AcquireEvidence,
        allow_terminal: bool,
        authority: MutationAuthority,
        change: impl FnOnce(
            &AdmissionRecord,
            &mut AcquisitionBudget,
            &mut AccountBudget,
            i64,
        ) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.bind(command)?;
        let mut tx = self.transaction().await?;
        if matches!(authority, MutationAuthority::OwnerConfirmation) {
            crate::qualification::require_owner(&mut tx).await?;
        }
        crate::retention::guard_request(&mut tx, &command.request).await?;
        let account = sqlx::query!(
            "SELECT account_id FROM edgeagent_acquisitions WHERE request_id=$1",
            command.request.as_str()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::storage)?
        .ok_or_else(|| Error::new(Kind::NotFound))?;
        let row = sqlx::query!(
            "SELECT budget,blocked FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            account.account_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let mut shared = AccountBudget::decode(&row.budget)?;
        let (record, mut budget) = load(&mut tx, command).await?;
        if row.blocked {
            return Err(Error::new(Kind::Denied));
        }
        if !allow_terminal && sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts WHERE request_id=$1) AS "present!""#,
            command.request.as_str()
        ).fetch_one(&mut *tx).await.map_err(Error::storage)? {
            return Err(Error::new(Kind::Terminal));
        }
        if format!("{:032x}", record.request().scope.account.get()) != account.account_id {
            return Err(Error::new(Kind::Invalid));
        }
        let now = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if matches!(authority, MutationAuthority::Qualification) {
            crate::qualification::guard(
                &mut tx,
                self.qualification.as_ref(),
                &record,
                Some((&budget, &shared)),
            )
            .await?;
            self.guard_qualification_worker(&mut tx, &record).await?;
        }
        // Approval contention must not preserve the earlier accounting sample.
        let now = if matches!(authority, MutationAuthority::Qualification) {
            sqlx::query_scalar!(
                r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::storage)?
        } else {
            now
        };
        let result = change(&record, &mut budget, &mut shared, now)?;
        save(&mut tx, command, &account.account_id, &budget, &shared).await?;
        if matches!(authority, MutationAuthority::Qualification) {
            crate::qualification::guard(&mut tx, self.qualification.as_ref(), &record, None)
                .await?;
        }
        tx.commit().await.map_err(Error::storage)?;
        Ok(result)
    }
}
pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    command: &AcquireEvidence,
) -> Result<(AdmissionRecord, AcquisitionBudget), Error> {
    crate::retention::guard_request(tx, &command.request).await?;
    let row = sqlx::query!(
        "SELECT digest, admission, budget FROM edgeagent_acquisitions
         WHERE request_id=$1 FOR UPDATE",
        command.request.as_str()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?;
    let Some(row) = row else {
        crate::retention::not_retired(tx, &command.request).await?;
        return Err(Error::new(Kind::NotFound));
    };
    if row.digest != command.request_sha256.as_str() {
        return Err(Error::new(Kind::Conflict));
    }
    let record = AdmissionRecord::decode(&row.admission, command)?;
    crate::retention::active(tx, &record).await?;
    let budget = AcquisitionBudget::decode(&row.budget)?;
    budget.verify(&record)?;
    Ok((record, budget))
}
async fn save(
    tx: &mut Transaction<'_, Postgres>,
    command: &AcquireEvidence,
    account: &str,
    budget: &AcquisitionBudget,
    shared: &AccountBudget,
) -> Result<(), Error> {
    let state = budget.encode()?;
    sqlx::query!(
        "UPDATE edgeagent_acquisitions SET budget=$2 WHERE request_id=$1",
        command.request.as_str(),
        state
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    let state = shared.encode()?;
    sqlx::query!(
        "UPDATE edgeagent_acquisition_accounts SET budget=$2 WHERE account_id=$1",
        account,
        state
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    Ok(())
}
impl AcquisitionLedger for PostgresAcquisitionLedger {
    async fn admit(
        &self,
        record: &AdmissionRecord,
        authority: &AuthorizedAcquisition<'_>,
    ) -> Result<AdmissionRecord, Error> {
        self.bind(record.command())?;
        record.verify_authority(authority)?;
        let account = format!("{:032x}", record.request().scope.account.get());
        let shared = AccountBudget::new(record.limits())?.encode()?;
        let mut tx = self.transaction().await?;
        crate::retention::not_retired(&mut tx, &record.command().request).await?;
        let retention_scope = crate::retention::scope_key(&record.retention_scope()?);
        sqlx::query!(
            "INSERT INTO edgeagent_evidence_scopes(scope) VALUES($1) ON CONFLICT DO NOTHING",
            retention_scope
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        crate::retention::active(&mut tx, record).await?;
        sqlx::query!(
            "INSERT INTO edgeagent_acquisition_accounts (account_id,budget)
             VALUES ($1,$2) ON CONFLICT DO NOTHING",
            account,
            shared
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let row = sqlx::query!(
            "SELECT budget,blocked FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            account
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if row.blocked {
            return Err(Error::new(Kind::Denied));
        }
        crate::retention::not_retired(&mut tx, &record.command().request).await?;
        // Sample after contention; a lock wait must not extend the admission deadline.
        let now = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if now < record.request().admitted_at.unix_millis() || now >= record.valid_until_ms()? {
            return Err(Error::new(Kind::Deadline));
        }
        let mut shared = AccountBudget::decode(&row.budget)?;
        shared.tighten(record.limits())?;
        let initial = AcquisitionBudget::default().encode()?;
        let delete_ms = record.delete_by_ms()?;
        let orphan_ms = record.valid_until_ms()?;
        sqlx::query!(
            "INSERT INTO edgeagent_acquisitions (request_id,account_id,digest,admission,budget,retention_scope,delete_ms,orphan_ms)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
            record.command().request.as_str(),
            account,
            record.command().request_sha256.as_str(),
            record.bytes(),
            initial, retention_scope, delete_ms, orphan_ms
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let (stored, state) = load(&mut tx, record.command()).await?;
        if stored.bytes() != record.bytes() {
            return Err(Error::new(Kind::Conflict));
        }
        save(&mut tx, record.command(), &account, &state, &shared).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(stored)
    }
    async fn load(
        &self,
        command: &AcquireEvidence,
    ) -> Result<(AdmissionRecord, BudgetSnapshot), Error> {
        self.bind(command)?;
        let mut tx = self.transaction().await?;
        let (record, state) = load(&mut tx, command).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok((record, state.snapshot()))
    }
    async fn begin_attempt(
        &self,
        command: &AcquireEvidence,
        attempt: u8,
    ) -> Result<BudgetSnapshot, Error> {
        self.mutate(
            command,
            false,
            MutationAuthority::Qualification,
            |r, b, _, now| {
                b.begin_attempt(r, attempt, now)?;
                Ok(b.snapshot())
            },
        )
        .await
    }
    async fn start_call(
        &self,
        command: &AcquireEvidence,
        traversal: Option<&EvidenceId>,
    ) -> Result<CallPermit, Error> {
        self.mutate(
            command,
            false,
            MutationAuthority::Qualification,
            |r, b, a, now| b.start(a, r, traversal, now),
        )
        .await
    }
    async fn complete_call(
        &self,
        permit: &CallPermit,
        outcome: CallCompletion,
    ) -> Result<BudgetSnapshot, Error> {
        let authority = if outcome == CallCompletion::QuiescedUnknown {
            MutationAuthority::OwnerConfirmation
        } else {
            MutationAuthority::Ordinary
        };
        self.mutate(permit.command(), true, authority, |r, b, a, now| {
            b.complete(a, r, permit, outcome, now)?;
            Ok(b.snapshot())
        })
        .await
    }
    async fn cancel(&self, command: &AcquireEvidence) -> Result<BudgetSnapshot, Error> {
        self.mutate(command, false, MutationAuthority::Ordinary, |_, b, _, _| {
            b.cancel();
            Ok(b.snapshot())
        })
        .await
    }
}
