use super::*;
impl Ledger {
    /// Install immutable complete reference/configuration bytes through the independent owner.
    /// The operator validates their provenance; SQL authentication does not certify legal facts.
    /// # Errors
    /// Rejects worker privileges, oversized input, changed retries or a missing admission.
    pub async fn install_qualification_input(
        &self,
        command: &AcquireEvidence,
        document: &[u8],
    ) -> Result<()> {
        if document.is_empty() || document.len() > 262144 {
            return Err(invalid(()));
        }
        let mut tx = self.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        ledger::load(&mut tx, command).await?;
        sqlx::query!("INSERT INTO edgeagent_qualification_inputs(request_id,document) VALUES($1,$2) ON CONFLICT DO NOTHING",command.request.as_str(),document)
            .execute(&mut *tx).await.map_err(BudgetError::storage)?;
        let old = sqlx::query_scalar!(
            "SELECT document FROM edgeagent_qualification_inputs WHERE request_id=$1",
            command.request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?;
        if old != document {
            return Err(BudgetError::new(BudgetErrorKind::Conflict));
        }
        tx.commit().await.map_err(BudgetError::storage)
    }
    /// Read only independently installed immutable input for this exact protected acquisition.
    /// # Errors
    /// Rejects missing/withdrawn approval, privileged workers and missing configuration.
    pub async fn qualification_input(&self) -> Result<Vec<u8>> {
        let p = self.qualification.as_ref().ok_or_else(denied)?;
        let mut tx = self.transaction().await?;
        let (r, _) = ledger::load(&mut tx, &p.data().admission).await?;
        qualification::guard(&mut tx, Some(p), &r, None).await?;
        let allowed =
            sqlx::query_scalar!(r#"SELECT edgeagent_qualification_input_role() AS "allowed!""#)
                .fetch_one(&mut *tx)
                .await
                .map_err(BudgetError::storage)?;
        if !allowed {
            return Err(denied());
        }
        let bytes = sqlx::query_scalar!(
            r#"SELECT edgeagent_qualification_input($1) AS "document""#,
            r.command().request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(BudgetError::storage)?
        .ok_or_else(denied)?;
        tx.commit().await.map_err(BudgetError::storage)?;
        Ok(bytes)
    }
}
