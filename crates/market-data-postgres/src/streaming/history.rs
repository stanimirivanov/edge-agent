//! Bounded metadata-first reconstruction; never fetch unbounded private preparations.
use super::*;
/// Opaque retained publication inventory, not read authority or a readiness grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamHistoryEntry {
    /// Original opaque intent.
    pub publication: EvidenceId,
    /// Exact private preparation size; inspect aggregate bounds before fetching bodies.
    pub preparation_bytes: usize,
    /// Confirmed existing publication receipt.
    pub published: bool,
    /// History has been erased; reconstruction must not invent the missing facts.
    pub retired: bool,
}
impl PostgresStreamingLedger {
    /// List at most 128 exact session intents, including tombstones. Exceeding the caller's
    /// positive limit fails closed rather than returning a silently incomplete history.
    /// # Errors
    /// Denies current authority/fence changes and rejects an incomplete bounded inventory.
    pub async fn stream_history(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
        limit: u16,
    ) -> Result<Vec<StreamHistoryEntry>> {
        if !(1..=128).contains(&limit) {
            return Err(invalid());
        }
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        recovery::enforce(&mut tx, session, self.fence.as_ref(), true).await?;
        let rows=sqlx::query!(r#"SELECT publication_id,COALESCE(octet_length(preparation),0) AS "bytes!",published,retired FROM edgeagent_stream_publications WHERE session_id=$1 ORDER BY publication_id LIMIT $2"#,session.data().id.as_str(),i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(Error::storage)?;
        if rows.len() > usize::from(limit) {
            return Err(Error::new(K::Exhausted));
        }
        let values = rows
            .into_iter()
            .map(|r| {
                Ok(StreamHistoryEntry {
                    publication: EvidenceId::new(&r.publication_id).map_err(|_| invalid())?,
                    preparation_bytes: usize::try_from(r.bytes).map_err(|_| invalid())?,
                    published: r.published,
                    retired: r.retired,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(values)
    }
}
