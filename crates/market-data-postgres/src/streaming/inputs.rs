//! Independent owner installation; no provider adapter or vendor DTO enters this crate.
use super::*;
use edgeagent_market_data_application::StreamOwnerInputs;
pub(super) async fn authenticate(
    tx: &mut Transaction<'_, Postgres>,
    session: &StreamingSession,
    expected: &StreamOwnerInputs,
) -> Result<()> {
    let bytes = sqlx::query_scalar!(
        r#"SELECT edgeagent_load_stream_inputs($1) AS "inputs?""#,
        session.data().id.as_str()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?
    .ok_or_else(denied)?;
    if bytes != expected.bytes() {
        return Err(conflict());
    }
    Ok(())
}
fn classify(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|c| matches!(c.as_ref(), "23502" | "23505" | "23514"))
    {
        Error::with_source(K::Conflict, error)
    } else {
        Error::storage(error)
    }
}
async fn control(
    tx: &mut Transaction<'_, Postgres>,
    id: &EvidenceId,
    action: &str,
    inputs: Option<&StreamOwnerInputs>,
) -> Result<()> {
    let body = inputs.map(StreamOwnerInputs::bytes);
    let expiry = inputs.map(|i| i.expires_at().unix_millis());
    let approved = sqlx::query_scalar!(
        r#"SELECT edgeagent_stream_owner_inputs_control($1,$2,$3,$4) AS "approved!""#,
        id.as_str(),
        action,
        body,
        expiry
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(classify)?;
    if !approved {
        return Err(denied());
    }
    Ok(())
}
impl PostgresStreamingLedger {
    /// Install complete exact owner inputs and reserve all logical namespaces atomically.
    /// The authenticated admission/account owner must perform this operation, not a worker.
    /// Same-byte retries never extend expiry or rewrite an existing namespace's scope.
    /// # Errors
    /// Denies expired/foreign inputs; conflicts roll back every reservation and membership.
    pub async fn install_stream_inputs(
        &self,
        session: &StreamingSession,
        inputs: &StreamOwnerInputs,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        lifecycle::owner_session(&mut tx, session.account(), &session.data().id).await?;
        inputs.recheck(
            session,
            catalogs,
            now(&mut tx).await?,
            &Cancellation::default(),
        )?;
        control(&mut tx, &session.data().id, "install", Some(inputs)).await?;
        inputs.recheck(
            session,
            catalogs,
            now(&mut tx).await?,
            &Cancellation::default(),
        )?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Load the actual protected owner record, decode it and compare complete candidate bytes.
    /// Callers re-derive candidates from typed provider profiles, not a parallel hand-written
    /// claim map. The returned snapshot is data, never a durable read/activation permission.
    /// # Errors
    /// Denies owner-capable workers, denial/expiry/retirement, changed candidates or claims.
    pub async fn load_stream_inputs(
        &self,
        session: &StreamingSession,
        expected: &StreamOwnerInputs,
        catalogs: AuthorityCatalogs<'_>,
        cancel: &Cancellation,
    ) -> Result<StreamOwnerInputs> {
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        expected.recheck(session, catalogs, now(&mut tx).await?, cancel)?;
        let bytes = sqlx::query_scalar!(
            r#"SELECT edgeagent_load_stream_inputs($1) AS "inputs?""#,
            session.data().id.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?
        .ok_or_else(denied)?;
        let actual = StreamOwnerInputs::decode(&bytes)?;
        if actual.bytes() != expected.bytes() {
            return Err(conflict());
        }
        actual.recheck(session, catalogs, now(&mut tx).await?, cancel)?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(actual)
    }
    /// Erase private input/scope metadata after independently confirmed storage-writer
    /// quiescence. Other active profiles keep shared scopes; opaque namespaces never reopen.
    /// This releases no provider connection, usage counter or immutable evidence object.
    /// # Errors
    /// Denies non-owner identities, unknown scope and unconfirmed writers.
    pub async fn retire_stream_inputs(
        &self,
        account: ProviderAccountId,
        id: &EvidenceId,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        lifecycle::owner_session(&mut tx, account, id).await?;
        control(&mut tx, id, "retire", None).await?;
        tx.commit().await.map_err(Error::storage)
    }
}
