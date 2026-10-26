//! Owner-only denial/retirement. Deletion is outside SQL and never releases source capacity.
use super::*;
use edgeagent_contracts::Component;
use edgeagent_market_data_application::{StreamCleanup, StreamDeletion, StreamDeletionAuthority};

impl PostgresStreamingLedger {
    /// Immediately deny retained reads/publication, independently of capture cancellation.
    /// # Errors
    /// Requires the independent owner and an exact account/session binding.
    pub async fn withdraw_stream_data(
        &self,
        account: ProviderAccountId,
        session: &EvidenceId,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        owner_session(&mut tx, account, session).await?;
        sqlx::query!(
            "UPDATE edgeagent_stream_retention SET denied=TRUE WHERE session_id=$1",
            session.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        stop(&mut tx, session).await?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Independent irreversible assertion: all storage PUT writers for this session are quiescent.
    /// Stops capture/publication without withdrawing already-published reads. This does not
    /// assert provider/socket quiescence and never releases its unresolved connection slot.
    /// # Errors
    /// Denies worker identities or mismatched owner scope.
    pub async fn confirm_stream_writers_quiescent(
        &self,
        account: ProviderAccountId,
        session: &EvidenceId,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        owner_session(&mut tx, account, session).await?;
        stop(&mut tx, session).await?;
        sqlx::query!(
            "UPDATE edgeagent_stream_retention SET writers_quiet=TRUE WHERE session_id=$1",
            session.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Retire an expired object or one belonging to a withdrawn session. Unpublished stopped
    /// preparations are eligible orphans. Never retires unexpired published data on stop alone.
    /// Atomically erases licensed preparation/receipt/head/outbox; keeps exact deletion inventory.
    /// # Errors
    /// Denies premature retirement, non-owner identities and unknown scope.
    pub async fn retire_stream_segment(
        &self,
        account: ProviderAccountId,
        session: &EvidenceId,
        id: &EvidenceId,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        owner_session(&mut tx, account, session).await?;
        let r=sqlx::query!("SELECT p.published,p.retired,p.delete_ms,r.denied,a.withdrawn FROM edgeagent_stream_publications p JOIN edgeagent_stream_retention r USING(session_id) JOIN edgeagent_stream_admissions a USING(session_id) WHERE p.publication_id=$1 AND p.session_id=$2 FOR UPDATE OF p",id.as_str(),session.as_str()).fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(conflict)?;
        if !r.retired
            && !r.denied
            && r.delete_ms > now(&mut tx).await?.unix_millis()
            && (r.published || !r.withdrawn)
        {
            return Err(denied());
        }
        // Unknown pre-PUT work holds its lane until capture is irreversibly stopped.
        if !r.retired && !r.published && !r.withdrawn {
            return Err(denied());
        }
        sqlx::query!(
            "UPDATE edgeagent_stream_heads SET digest=NULL WHERE publication_id=$1",
            id.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        sqlx::query!(
            "DELETE FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2",
            Component::MarketData.source_uri(),
            format!("stream-publication-{}", id.as_str())
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        sqlx::query!("UPDATE edgeagent_stream_publications SET retired=TRUE,preparation=NULL,event=NULL,instrument=NULL,history_start=NULL,history_retired=NULL,reconnect=NULL,skipped=NULL WHERE publication_id=$1",id.as_str()).execute(&mut *tx).await.map_err(Error::storage)?;
        erase_closed_metadata(&mut tx, session).await?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Bounded owner-only orphan cleanup; exact descriptors survive failed/uncertain deletion.
    /// Independent per-object confirmation supplements durable session writer quiescence.
    /// # Errors
    /// Denies unconfirmed writers, invalid limits (1..=16), changed inventory or storage failure.
    pub async fn cleanup_stream_segments(
        &self,
        account: ProviderAccountId,
        session: &EvidenceId,
        limit: u32,
        store: &impl StreamDeletion,
        authority: &impl StreamDeletionAuthority,
    ) -> Result<usize> {
        if !(1..=16).contains(&limit) {
            return Err(invalid());
        }
        let mut tx = self.ledger.transaction().await?;
        owner_session(&mut tx, account, session).await?;
        let quiet = sqlx::query_scalar!(
            "SELECT writers_quiet FROM edgeagent_stream_retention WHERE session_id=$1 FOR SHARE",
            session.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if !quiet {
            return Err(denied());
        }
        let rows=sqlx::query!("SELECT publication_id,descriptor FROM edgeagent_stream_publications WHERE session_id=$1 AND retired AND descriptor<>'' ORDER BY publication_id LIMIT $2",session.as_str(),i64::from(limit)).fetch_all(&mut *tx).await.map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)?;
        let mut completed = 0;
        for row in rows {
            let cleanup = StreamCleanup {
                publication: EvidenceId::new(&row.publication_id).map_err(|_| invalid())?,
                object: serde_json::from_str(&row.descriptor).map_err(|_| invalid())?,
            };
            store
                .delete_segment(&cleanup, authority)
                .await
                .map_err(|e| Error::with_source(K::Storage, e))?;
            authority
                .confirm_storage_quiescent(&cleanup)
                .map_err(|e| Error::with_source(K::Denied, e))?;
            let mut tx = self.ledger.transaction().await?;
            owner_session(&mut tx, account, session).await?;
            let removed=sqlx::query!("UPDATE edgeagent_stream_publications SET descriptor='',delete_ms=0,published=FALSE WHERE publication_id=$1 AND session_id=$2 AND retired AND descriptor=$3",row.publication_id,session.as_str(),row.descriptor).execute(&mut *tx).await.map_err(Error::storage)?;
            completed += usize::try_from(removed.rows_affected()).map_err(|_| invalid())?;
            // Preserve opaque denial/capacity/counters, not licensed full admission witnesses.
            erase_closed_metadata(&mut tx, session).await?;
            tx.commit().await.map_err(Error::storage)?;
        }
        Ok(completed)
    }
}
async fn owner_session(
    tx: &mut Transaction<'_, Postgres>,
    account: ProviderAccountId,
    id: &EvidenceId,
) -> Result<()> {
    qualification::require_owner(tx).await?;
    lock_account(tx, &key(account)).await?;
    let valid=sqlx::query_scalar!(r#"SELECT account_id=$2 AS "valid!" FROM edgeagent_stream_admissions WHERE session_id=$1 FOR UPDATE"#,id.as_str(),key(account)).fetch_optional(&mut **tx).await.map_err(Error::storage)?;
    if valid != Some(true) {
        return Err(denied());
    }
    state(tx, id).await?;
    Ok(())
}
async fn stop(tx: &mut Transaction<'_, Postgres>, id: &EvidenceId) -> Result<()> {
    sqlx::query!(
        "UPDATE edgeagent_stream_admissions SET withdrawn=TRUE WHERE session_id=$1",
        id.as_str()
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    sqlx::query!(
        "UPDATE edgeagent_stream_sessions SET stopped=TRUE WHERE session_id=$1",
        id.as_str()
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    erase_closed_metadata(tx, id).await
}

// Caller already holds owner account/admission/session locks. Never erase live publications.
pub(super) async fn erase_closed_metadata(
    tx: &mut Transaction<'_, Postgres>,
    id: &EvidenceId,
) -> Result<()> {
    sqlx::query!("UPDATE edgeagent_stream_admissions a SET admission=decode('00','hex') WHERE session_id=$1 AND withdrawn AND NOT EXISTS(SELECT 1 FROM edgeagent_stream_publications p WHERE p.session_id=a.session_id AND NOT p.retired)",id.as_str()).execute(&mut **tx).await.map_err(Error::storage)?;
    sqlx::query!("DELETE FROM edgeagent_stream_heads h USING edgeagent_stream_admissions a WHERE h.session_id=a.session_id AND a.session_id=$1 AND a.withdrawn AND NOT EXISTS(SELECT 1 FROM edgeagent_stream_publications p WHERE p.session_id=a.session_id AND NOT p.retired)",id.as_str()).execute(&mut **tx).await.map_err(Error::storage)?;
    Ok(())
}
