//! Independent owner writes; worker checks run within existing mutation transactions.
use crate::{PostgresAcquisitionLedger, ledger};
use edgeagent_contracts::evidence::EvidenceVersion;
use edgeagent_market_data_application::{
    AccountBudget, AcquisitionBudget, AdmissionRecord, BudgetError as Error, BudgetErrorKind as K,
    QualificationControl, QualificationControlReview, QualificationProfile,
};
use edgeagent_market_data_domain::UtcInstant;
use sqlx::{Postgres, Transaction};

type Reviews = [(EvidenceVersion, i64, i64); 5];
const CONTROLS: [QualificationControl; 5] = [
    QualificationControl::DataOnlyCredentialScope,
    QualificationControl::ReferenceProvenance,
    QualificationControl::RuntimeFencing,
    QualificationControl::RetentionAndCleanup,
    QualificationControl::RestoreAndConsumers,
];

fn invalid(cause: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::with_source(K::Invalid, cause)
}
pub(super) async fn require_owner(tx: &mut Transaction<'_, Postgres>) -> Result<(), Error> {
    if !sqlx::query_scalar!(
        r#"SELECT owner_allowed AS "allowed!" FROM edgeagent_qualification_role()"#
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?
    {
        return Err(Error::new(K::Denied));
    }
    Ok(())
}
async fn now(tx: &mut Transaction<'_, Postgres>) -> Result<UtcInstant, Error> {
    let ms = sqlx::query_scalar!(
        r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    UtcInstant::from_unix_millis(ms).map_err(invalid)
}
// Also reject NOINHERIT membership that could SET ROLE to a control writer, and
// schema CREATE that could substitute relations. This is not application RBAC.
pub(super) async fn owner_capable(tx: &mut Transaction<'_, Postgres>) -> Result<bool, Error> {
    sqlx::query_scalar!(
        r#"SELECT NOT worker_allowed AS "capable!" FROM edgeagent_qualification_role()"#
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)
}
fn validate_reviews(reviews: &Reviews, now: UtcInstant, delete_by: i64) -> Result<(), Error> {
    for (_, known, until) in reviews {
        let known = UtcInstant::from_unix_millis(*known).map_err(invalid)?;
        let until = UtcInstant::from_unix_millis(*until).map_err(invalid)?;
        if known > now || until <= now || until.unix_millis() < delete_by {
            return Err(Error::new(K::Denied));
        }
    }
    Ok(())
}
impl PostgresAcquisitionLedger {
    /// Record the independently authenticated owner's five control reviews, in documented order.
    /// The SQL role must have INSERT/UPDATE privilege on the protected control relation.
    /// A profile/review DTO alone establishes no permission: the operator verifies external
    /// legal, credential-scope, reference, fencing and cleanup facts before this call.
    /// Exact retries are idempotent; withdrawal is irreversible for this acquisition.
    /// # Errors
    /// Rejects insufficient SQL privileges, changed binding/reviews, invalid windows or retirement.
    pub async fn approve_qualification(
        &self,
        profile: &QualificationProfile,
        controls: &[QualificationControlReview; 5],
    ) -> Result<(), Error> {
        let mut tx = self.transaction().await?;
        require_owner(&mut tx).await?;
        let (record, _) = ledger::load(&mut tx, &profile.data().admission).await?;
        let now = now(&mut tx).await?;
        profile.validate_admission(&record, now)?;
        let mut reviews = Vec::with_capacity(5);
        for (control, expected) in controls.iter().zip(CONTROLS) {
            if control.control != expected {
                return Err(Error::new(K::Invalid));
            }
            reviews.push((
                control.evidence.clone(),
                control.known_at.unix_millis(),
                control.expires_at.unix_millis(),
            ));
        }
        let reviews: Reviews = reviews.try_into().map_err(|_| Error::new(K::Invalid))?;
        validate_reviews(&reviews, now, record.delete_by_ms()?)?;
        let bytes = serde_json::to_vec(&reviews).map_err(invalid)?;
        if bytes.len() > 4096 {
            return Err(Error::new(K::Invalid));
        }
        sqlx::query!("INSERT INTO edgeagent_qualification_grants(request_id,profile,reviews,approved) VALUES($1,$2,$3,TRUE) ON CONFLICT DO NOTHING",
            profile.data().admission.request.as_str(), profile.bytes(), bytes)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        let existing = sqlx::query!("SELECT profile,reviews,approved FROM edgeagent_qualification_grants WHERE request_id=$1 FOR SHARE",
            profile.data().admission.request.as_str()).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !existing.approved {
            return Err(Error::new(K::Denied));
        }
        if existing.profile != profile.bytes() || existing.reviews != bytes {
            return Err(Error::new(K::Conflict));
        }
        tx.commit().await.map_err(Error::storage)
    }

    /// Permanently withdraw this exact profile; races serialize with guarded starts/publication.
    /// Does not clear pending calls, refund usage, delete objects or manufacture quiescence.
    /// # Errors
    /// Rejects missing/mismatched profiles and insufficient SQL owner privileges.
    pub async fn withdraw_qualification(
        &self,
        profile: &QualificationProfile,
    ) -> Result<(), Error> {
        let mut tx = self.transaction().await?;
        require_owner(&mut tx).await?;
        let changed = sqlx::query!("UPDATE edgeagent_qualification_grants SET approved=FALSE WHERE request_id=$1 AND profile=$2",
            profile.data().admission.request.as_str(),profile.bytes())
            .execute(&mut *tx).await.map_err(Error::storage)?;
        if changed.rows_affected() != 1 {
            return Err(Error::new(K::Conflict));
        }
        tx.commit().await.map_err(Error::storage)
    }
}

/// Scope/account/command locks precede this approval lock. Owner withdrawal takes only
/// the approval lock, so it cannot invert retention/account lock order. A successful
/// reservation linearizes before withdrawal or is denied after it, never in between.
pub(super) async fn guard(
    tx: &mut Transaction<'_, Postgres>,
    profile: Option<&QualificationProfile>,
    record: &AdmissionRecord,
    budget: Option<(&AcquisitionBudget, &AccountBudget)>,
) -> Result<(), Error> {
    // Only this bounded locking read is SECURITY DEFINER. Workers receive no
    // UPDATE privilege just to satisfy PostgreSQL's SELECT FOR SHARE requirement.
    let row = sqlx::query!(r#"SELECT profile AS "profile!",reviews AS "reviews!",approved AS "approved!",independent AS "independent!" FROM edgeagent_lock_qualification_grant($1)"#,
        record.command().request.as_str()).fetch_optional(&mut **tx).await.map_err(Error::storage)?;
    let Some(profile) = profile else {
        // Existing invented/local owner workflows remain usable. A worker role or
        // a protected acquisition cannot bypass binding by using the legacy constructor.
        if row.is_some() {
            return Err(Error::new(K::Denied));
        }
        return require_owner(tx).await;
    };
    if owner_capable(tx).await? {
        return Err(Error::new(K::Denied));
    }
    let row = row
        .filter(|r| r.approved && r.independent)
        .ok_or_else(|| Error::new(K::Denied))?;
    if row.profile != profile.bytes() {
        return Err(Error::new(K::Conflict));
    }
    let now = now(tx).await?;
    profile.validate_admission(record, now)?;
    if row.reviews.len() > 4096 {
        return Err(Error::new(K::Invalid));
    }
    let reviews: Reviews = serde_json::from_slice(&row.reviews).map_err(invalid)?;
    if serde_json::to_vec(&reviews).map_err(invalid)? != row.reviews {
        return Err(Error::new(K::Invalid));
    }
    validate_reviews(&reviews, now, record.delete_by_ms()?)?;
    if let Some((command, account)) = budget {
        if account.has_pending_call() {
            return Err(Error::new(K::Busy));
        }
        if !account.matches_daily_policy(&profile.data().credits) {
            return Err(Error::new(K::Conflict));
        }
        profile.validate_remaining_budget(
            record,
            &command.snapshot(),
            account.daily_usage().ok_or_else(|| Error::new(K::Denied))?,
            now,
        )?;
    }
    Ok(())
}
