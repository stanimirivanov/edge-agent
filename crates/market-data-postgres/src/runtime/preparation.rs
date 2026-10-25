use super::*;
impl Ledger {
    /// Retain exact accepted precise bytes before PUT; changed retries conflict.
    /// # Errors
    /// Rejects mismatched/current authority or immutable preparation identity.
    pub async fn stage_precise_preparation(&self, artifact: &DailyEvidenceArtifact) -> Result<()> {
        let d = artifact.data();
        let command = AcquireEvidence {
            request: d.scope.request.clone(),
            request_sha256: d.scope.request_sha256.clone(),
        };
        self.bind(&command)?;
        let mut tx = self.transaction().await?;
        let (r, _) = ledger::load(&mut tx, &command).await?;
        r.verify_artifact(artifact)?;
        qualification::guard(&mut tx, self.qualification.as_ref(), &r, None).await?;
        self.guard_qualification_worker(&mut tx, &r).await?;
        sqlx::query!("INSERT INTO edgeagent_precise_preparations(request_id,digest,artifact) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",command.request.as_str(),artifact.digest().as_str(),artifact.bytes())
            .execute(&mut *tx).await.map_err(BudgetError::storage)?;
        let old = sqlx::query!(
            "SELECT digest,artifact FROM edgeagent_precise_preparations WHERE request_id=$1",
            command.request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        if old.digest != artifact.digest().as_str() || old.artifact != artifact.bytes() {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
    /// Recover exact precise acceptance before new source work. Applies current durable denial.
    /// # Errors
    /// Rejects missing authority or corruption, never substitutes new timestamps/identities.
    pub async fn recover_precise_preparation(
        &self,
        command: &AcquireEvidence,
        access: &impl EvidenceAccess,
    ) -> Result<Option<DailyEvidenceArtifact>> {
        self.bind(command)?;
        let mut tx = self.transaction().await?;
        let (r, _) = ledger::load(&mut tx, command).await?;
        let stored = sqlx::query!(
            "SELECT digest,artifact FROM edgeagent_precise_preparations WHERE request_id=$1",
            command.request.as_str()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        let result = stored
            .map(|s| -> Result<_> {
                let a = DailyEvidenceArtifact::decode(
                    &s.artifact,
                    &EvidenceDigest::new(&s.digest).map_err(invalid)?,
                )
                .map_err(invalid)?;
                r.verify_artifact(&a)?;
                let d = EvidenceDecision::ready(command.clone(), &a).map_err(invalid)?;
                let EvidenceOutcome::Ready { object } = &d.data().outcome else {
                    return Err(invalid(()));
                };
                access
                    .authorize(object)
                    .map_err(|e| BudgetError::with_source(BudgetErrorKind::Denied, e))?;
                Ok(a)
            })
            .transpose()?;
        tx.commit().await.map_err(BudgetError::storage)?;
        Ok(result)
    }
}
