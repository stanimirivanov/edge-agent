use super::*;
/// Independently owner-confirmed exact retired inventory. No account-unblock capability.
pub struct QualificationCleanup {
    request: EvidenceId,
    objects: Vec<EvidenceObject>,
}
impl std::fmt::Debug for QualificationCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QualificationCleanup(..)")
    }
}
impl CleanupAuthority for QualificationCleanup {
    fn authorize_deletion(&self, r: &EvidenceId, o: &EvidenceObject) -> Result<()> {
        self.confirm_writes_quiescent(r)?;
        if !self.objects.contains(o) {
            return Err(denied());
        }
        Ok(())
    }
    fn confirm_writes_quiescent(&self, r: &EvidenceId) -> Result<()> {
        if r != &self.request {
            return Err(denied());
        }
        Ok(())
    }
    fn confirm_account_quiescent(&self, _: &EvidenceId, _: i64) -> Result<()> {
        Err(denied())
    }
}
impl Ledger {
    /// Approve exact deletion inventory after independently confirming physical writer quiescence.
    /// Never infers that fact from a timeout or an inactive marker. Owner SQL identity is mandatory.
    /// # Errors
    /// Rejects active admissions, unknown inventory and changes to already approved descriptors.
    pub async fn approve_qualification_cleanup(
        &self,
        item: &EvidenceCleanup,
        evidence: &EvidenceVersion,
    ) -> Result<()> {
        if item.objects.len() > 12 {
            return Err(invalid(()));
        }
        let bytes = serde_json::to_vec(&item.objects).map_err(invalid)?;
        let review = serde_json::to_string(evidence).map_err(invalid)?;
        let mut tx = self.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let retired=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisition_denials WHERE request_id=$1) AS "retired!""#,item.request.as_str())
            .fetch_one(&mut *tx).await.map_err(BudgetError::storage)?;
        if !retired {
            return Err(denied());
        }
        let rows=sqlx::query!("SELECT descriptor FROM edgeagent_evidence_objects WHERE request_id=$1 ORDER BY object_key FOR SHARE",item.request.as_str())
            .fetch_all(&mut *tx).await.map_err(BudgetError::storage)?;
        let current = rows
            .into_iter()
            .map(|r| serde_json::from_slice::<EvidenceObject>(&r.descriptor).map_err(invalid))
            .collect::<Result<Vec<_>>>()?;
        if current != item.objects {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        sqlx::query!("INSERT INTO edgeagent_qualification_cleanup(request_id,inventory,evidence) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",item.request.as_str(),bytes,review)
            .execute(&mut *tx).await.map_err(BudgetError::storage)?;
        let old = sqlx::query_scalar!(
            "SELECT inventory FROM edgeagent_qualification_cleanup WHERE request_id=$1",
            item.request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        if old != bytes {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
    /// Authenticate an owner's exact immutable cleanup approval. Call immediately before deletion.
    /// # Errors
    /// Rejects absent approval and privileged boundary failures. Does not release account blocks.
    pub async fn qualification_cleanup(
        &self,
        request: &EvidenceId,
    ) -> Result<QualificationCleanup> {
        let mut tx = self.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let bytes = sqlx::query_scalar!(
            "SELECT inventory FROM edgeagent_qualification_cleanup WHERE request_id=$1",
            request.as_str()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(BudgetError::storage)?
        .ok_or_else(denied)?;
        let objects = serde_json::from_slice::<Vec<EvidenceObject>>(&bytes).map_err(invalid)?;
        if objects.len() > 12 {
            return Err(invalid(()));
        }
        tx.commit().await.map_err(BudgetError::storage)?;
        Ok(QualificationCleanup {
            request: request.clone(),
            objects,
        })
    }
}
