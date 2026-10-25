use super::*;
use std::num::NonZeroU64;
/// Fresh authenticated PostgreSQL assessment. Not a reservation or reusable grant.
pub struct QualificationSnapshot {
    profile: QualificationProfile,
    record: AdmissionRecord,
    assessment: QualificationAssessment,
}
impl std::fmt::Debug for QualificationSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QualificationSnapshot(..)")
    }
}
impl QualificationSnapshot {
    /// Database clock sample to use for this single preflight evaluation.
    #[must_use]
    pub const fn at(&self) -> UtcInstant {
        self.assessment.checked_at
    }
}
impl QualificationAuthority for QualificationSnapshot {
    fn assess(
        &self,
        p: &QualificationProfile,
        r: &AdmissionRecord,
        at: UtcInstant,
    ) -> Result<Option<QualificationAssessment>> {
        if p != &self.profile || r != &self.record || at != self.at() {
            return Err(denied());
        }
        Ok(Some(self.assessment.clone()))
    }
}
impl Ledger {
    /// Load an authenticated consistent account/command/approval assessment without mutation.
    /// # Errors
    /// Denies unknown approval, terminal work, policy mismatch or insufficient credits.
    pub async fn qualification_snapshot(&self) -> Result<QualificationSnapshot> {
        let p = self.qualification.as_ref().ok_or_else(denied)?;
        let mut tx = self.transaction().await?;
        // Follow the mutation lock order: scope, account, command, approval.
        crate::retention::guard_request(&mut tx, &p.data().admission.request).await?;
        let account=sqlx::query!("SELECT a.budget,a.blocked,a.block_generation FROM edgeagent_acquisition_accounts a JOIN edgeagent_acquisitions q ON q.account_id=a.account_id WHERE q.request_id=$1 FOR SHARE OF a",p.data().admission.request.as_str())
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        let shared = AccountBudget::decode(&account.budget)?;
        let (r, b) = ledger::load(&mut tx, &p.data().admission).await?;
        qualification::guard(&mut tx, Some(p), &r, Some((&b, &shared))).await?;
        let terminal=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts WHERE request_id=$1) AS "present!""#,r.command().request.as_str())
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        if terminal {
            return Err(BudgetError::new(BudgetErrorKind::Terminal));
        }
        let grant = sqlx::query!(
            r#"SELECT reviews AS "reviews!" FROM edgeagent_lock_qualification_grant($1)"#,
            r.command().request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        let reviews: [(EvidenceVersion, i64, i64); 5] =
            serde_json::from_slice(&grant.reviews).map_err(invalid)?;
        use QualificationControl::*;
        let controls = [
            DataOnlyCredentialScope,
            ReferenceProvenance,
            RuntimeFencing,
            RetentionAndCleanup,
            RestoreAndConsumers,
        ]
        .into_iter()
        .zip(reviews)
        .map(|(control, (evidence, known, until))| {
            Ok(QualificationControlReview {
                control,
                evidence,
                known_at: UtcInstant::from_unix_millis(known).map_err(invalid)?,
                expires_at: UtcInstant::from_unix_millis(until).map_err(invalid)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let ms = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        let at = UtcInstant::from_unix_millis(ms).map_err(invalid)?;
        let account_state = if account.blocked {
            QualificationAccountState::Blocked(
                NonZeroU64::new(u64::try_from(account.block_generation).map_err(invalid)?)
                    .ok_or_else(denied)?,
            )
        } else if shared.has_pending_call() {
            QualificationAccountState::Pending
        } else {
            QualificationAccountState::Quiescent
        };
        let assessment = QualificationAssessment {
            profile_sha256: p.digest().clone(),
            owner_review: p.data().owner_review.clone(),
            checked_at: at,
            expires_at: controls
                .iter()
                .map(|c| c.expires_at)
                .min()
                .ok_or_else(denied)?,
            account: r.request().scope.account,
            credits: p.data().credits.clone(),
            usage: shared.daily_usage().ok_or_else(denied)?,
            budget: b.snapshot(),
            account_state,
            controls,
        };
        tx.commit().await.map_err(BudgetError::storage)?;
        Ok(QualificationSnapshot {
            profile: p.clone(),
            record: r,
            assessment,
        })
    }
}
