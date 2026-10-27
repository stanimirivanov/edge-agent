//! Protected monotonic restore control, independent from capture and retained-data rights.
use super::*;
use edgeagent_market_data_application::{
    StreamRecoveryLedger, StreamRecoveryState, StreamRestore, StreamStop,
};

fn revision(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid())
}
fn incident(value: i16) -> Result<Option<StreamStop>> {
    Ok(match value {
        0 => None,
        1 => Some(StreamStop::Overload),
        2 => Some(StreamStop::SourceFailure),
        3 => Some(StreamStop::Cancelled),
        4 => Some(StreamStop::Binding),
        _ => return Err(invalid()),
    })
}
fn decode(value: &str) -> Result<StreamRecoveryState> {
    if value.len() > 64 {
        return Err(invalid());
    }
    let mut parts = value.split(':');
    let revision: u64 = parts
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let blocked: bool = parts
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let incident = incident(
        parts
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?,
    )?;
    if parts.next().is_some() || (incident.is_some() && !blocked) {
        return Err(invalid());
    }
    Ok(StreamRecoveryState {
        revision,
        blocked,
        incident,
    })
}
async fn control(
    tx: &mut Transaction<'_, Postgres>,
    session: &StreamingSession,
    expected: u64,
    action: &str,
    reason: Option<i16>,
) -> Result<StreamRecoveryState> {
    let value = sqlx::query_scalar!(
        r#"SELECT edgeagent_stream_recovery_control($1,$2,$3,$4) AS "state?""#,
        session.data().id.as_str(),
        revision(expected)?,
        action,
        reason
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?
    .ok_or_else(denied)?;
    decode(&value)
}
impl PostgresStreamingLedger {
    /// Inspect protected state under current retained authority; never grants readiness.
    /// # Errors
    /// Denies owner-capable worker identities, missing authority or invalid state.
    pub async fn recovery_state(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<StreamRecoveryState> {
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        let state = control(&mut tx, session, 0, "inspect", None).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(state)
    }
    /// Begin a new restore and fence all prior revision witnesses before external work.
    /// Lost confirmation remains blocked. Re-entry never clears an incident or refunds usage.
    /// # Errors
    /// Denies sticky incidents and invalid current retained authority; another begin
    /// fences earlier attempts rather than granting concurrent restore permission.
    pub async fn begin_restore(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<StreamRestore> {
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        let old = control(&mut tx, session, 0, "inspect", None).await?;
        let state = control(&mut tx, session, old.revision, "begin", None).await?;
        tx.commit().await.map_err(Error::storage)?;
        StreamRestore::from_committed(session.data().id.clone(), state.revision)
    }
    /// Append a sticky session incident under the exact restore revision. Never clears
    /// rights or erases captures; reconciliation needs a separately admitted session.
    /// # Errors
    /// Denies stale/foreign witnesses or another already-recorded incident.
    pub async fn record_stream_incident(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        reason: StreamStop,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<StreamRecoveryState> {
        if restore.identity().0 != &session.data().id {
            return Err(invalid());
        }
        let reason = match reason {
            StreamStop::Overload => 1,
            StreamStop::SourceFailure => 2,
            StreamStop::Cancelled => 3,
            StreamStop::Binding => 4,
            _ => return Err(invalid()),
        };
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        let state = control(
            &mut tx,
            session,
            restore.identity().1,
            "incident",
            Some(reason),
        )
        .await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(state)
    }
    /// Independent owner's assertion that restore inputs/consumer state are current and
    /// prior workers are fenced. Approve only the exact revision with no pending intents.
    /// This is not evidence readiness and releases no provider/storage capacity.
    /// # Errors
    /// Denies worker LOGINs, stale revisions, incidents, pending work or expired authority.
    pub async fn approve_stream_restore(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<()> {
        if restore.identity().0 != &session.data().id {
            return Err(invalid());
        }
        let mut tx = self.ledger.transaction().await?;
        lifecycle::owner_session(&mut tx, session.account(), &session.data().id).await?;
        session.recheck(catalogs, now(&mut tx).await?, &Cancellation::default())?;
        control(&mut tx, session, restore.identity().1, "approve", None).await?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Fresh barrier check at use time, not a cached or serializable readiness token.
    /// PR20C must combine this with current authority and existing domain readiness.
    /// # Errors
    /// Denies unapproved/stale/incident sessions and any unresolved pending publication.
    pub async fn check_stream_restored(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<()> {
        self.check_recovery(session, restore, catalogs, "check")
            .await
    }
    async fn check_recovery(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        catalogs: AuthorityCatalogs<'_>,
        action: &str,
    ) -> Result<()> {
        if restore.identity().0 != &session.data().id {
            return Err(invalid());
        }
        let mut tx = self.ledger.transaction().await?;
        publication::retained_guard(&mut tx, session, catalogs).await?;
        if action == "check" {
            session.recheck(catalogs, now(&mut tx).await?, &Cancellation::default())?;
        }
        control(&mut tx, session, restore.identity().1, action, None).await?;
        tx.commit().await.map_err(Error::storage)
    }
}
impl StreamRecoveryLedger for PostgresStreamingLedger {
    async fn check_restoring(
        &self,
        session: &StreamingSession,
        restore: &StreamRestore,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<()> {
        self.check_recovery(session, restore, catalogs, "recover")
            .await
    }
}
