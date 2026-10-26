//! Account -> protected admission -> session -> retention -> lane; no object I/O in SQL.
use super::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::{
    StreamCaptureBoundary, StreamPublication, StreamPublicationLedger,
};
use edgeagent_outbox_postgres::PostgresOutbox;

pub(super) fn millis(t: EvidenceTime) -> Result<i64> {
    i64::try_from(i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000))
        .map_err(|_| invalid())
}
fn number(n: &EvidenceRevision) -> Result<i64> {
    n.as_str().parse().map_err(|_| invalid())
}
pub(super) async fn retained_guard(
    tx: &mut Transaction<'_, Postgres>,
    session: &StreamingSession,
    catalogs: AuthorityCatalogs<'_>,
) -> Result<()> {
    let allowed = sqlx::query_scalar!(r#"SELECT edgeagent_stream_worker_allowed() AS "allowed!""#)
        .fetch_one(&mut **tx)
        .await
        .map_err(Error::storage)?;
    if !allowed {
        return Err(denied());
    }
    let policy = lock_account(tx, &key(session.account())).await?;
    if policy.withdrawn {
        return Err(denied());
    }
    let admission=sqlx::query!(r#"SELECT account_id,admission=$2 AS "same!",independent FROM edgeagent_lock_stream_admission($1)"#,session.data().id.as_str(),session.bytes()).fetch_optional(&mut **tx).await.map_err(Error::storage)?.ok_or_else(denied)?;
    if !admission.same
        || admission.independent != Some(true)
        || admission.account_id.as_deref() != Some(key(session.account()).as_str())
    {
        return Err(denied());
    }
    state(tx, &session.data().id).await?;
    let row = sqlx::query!(
        "SELECT denied,writers_quiet,independent FROM edgeagent_lock_stream_retention($1)",
        session.data().id.as_str()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?
    .ok_or_else(denied)?;
    if row.denied != Some(false) || row.independent != Some(true) {
        return Err(denied());
    }
    session.recheck_retained(catalogs, now(tx).await?)
}
async fn write_guard(
    tx: &mut Transaction<'_, Postgres>,
    session: &StreamingSession,
    proposal: &StreamPublication,
    catalogs: AuthorityCatalogs<'_>,
    cancel: &Cancellation,
) -> Result<()> {
    retained_guard(tx, session, catalogs).await?;
    worker(tx, session).await?;
    session.recheck(catalogs, now(tx).await?, cancel)?;
    let d = proposal.segment().data();
    let row=sqlx::query!("SELECT generation,generation_start,frames,bytes,last_frame_bytes,active FROM edgeagent_stream_sessions WHERE session_id=$1",session.data().id.as_str()).fetch_one(&mut **tx).await.map_err(Error::storage)?;
    let quiet = sqlx::query_scalar!(
        r#"SELECT writers_quiet AS "quiet!" FROM edgeagent_lock_stream_retention($1)"#,
        session.data().id.as_str()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    let start = number(&d.frames[0])?;
    let end = number(&d.frames[1])?;
    if quiet
        || !row.active
        || row.generation != number(&d.generation)?
        || row.generation_start.is_none_or(|first| start < first)
        || end > row.frames
        || (end == row.frames
            && (u64::try_from(row.bytes).map_err(|_| invalid())?
                > session.data().limits.bytes.get()
                || row
                    .last_frame_bytes
                    .is_some_and(|n| n > i64::from(session.data().limits.frame_bytes.get()))))
        || d.scope.session != session.data().id
        || d.scope.session_sha256 != DailyEvidenceArtifact::hash(session.bytes())
        || millis(d.delete_by)? <= now(tx).await?.unix_millis()
    {
        return Err(denied());
    }
    proposal.recheck_capture(session, catalogs, now(tx).await?, cancel)
}

impl StreamPublicationLedger for PostgresStreamingLedger {
    async fn pending_segment(
        &self,
        session: &StreamingSession,
        instrument: edgeagent_market_data_domain::InstrumentId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<Option<StreamPublication>> {
        let mut tx = self.ledger.transaction().await?;
        retained_guard(&mut tx, session, catalogs).await?;
        let instrument = format!("{:032x}", instrument.get());
        let row=sqlx::query!("SELECT publication_id,preparation,delete_ms FROM edgeagent_stream_publications WHERE session_id=$1 AND instrument=$2 AND NOT published AND NOT retired",session.data().id.as_str(),instrument).fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        let value = if let Some(r) = row {
            if r.delete_ms <= now(&mut tx).await?.unix_millis() {
                return Err(denied());
            }
            let p = StreamPublication::decode(r.preparation.as_deref().ok_or_else(invalid)?)?;
            if p.id().as_str() != r.publication_id
                || p.segment().data().scope.session != session.data().id
            {
                return Err(invalid());
            }
            Some(p)
        } else {
            None
        };
        tx.commit().await.map_err(Error::storage)?;
        Ok(value)
    }
    async fn recover_publication(
        &self,
        session: &StreamingSession,
        p: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<Option<StreamCaptureBoundary>> {
        let mut tx = self.ledger.transaction().await?;
        retained_guard(&mut tx, session, catalogs).await?;
        let bytes = p.bytes()?;
        let row=sqlx::query!(r#"SELECT preparation=$3 AS "same!",event,history_start,history_retired,reconnect,skipped,delete_ms FROM edgeagent_stream_publications WHERE publication_id=$1 AND session_id=$2 AND published AND NOT retired"#,p.id().as_str(),session.data().id.as_str(),bytes).fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        let Some(r) = row else {
            return Ok(None);
        };
        let event = p.envelope().to_json().map_err(|_| invalid())?;
        if !r.same || r.event.as_deref() != Some(event.as_slice()) {
            return Err(conflict());
        }
        if r.delete_ms <= now(&mut tx).await?.unix_millis() {
            return Err(denied());
        }
        let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3) AS "intact!""#,p.envelope().source(),p.envelope().id(),event).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !intact {
            return Err(invalid());
        }
        let result = StreamCaptureBoundary {
            history_start: r.history_start.ok_or_else(invalid)?,
            history_retired: r.history_retired.ok_or_else(invalid)?,
            reconnect: r.reconnect.ok_or_else(invalid)?,
            skipped: r.skipped.ok_or_else(invalid)?,
        };
        tx.commit().await.map_err(Error::storage)?;
        Ok(Some(result))
    }
    async fn stage(
        &self,
        session: &StreamingSession,
        p: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
        cancel: &Cancellation,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        write_guard(&mut tx, session, p, catalogs, cancel).await?;
        // Reject forks before PUT: otherwise an orphan could alias a published object's key.
        let d = p.segment().data();
        let previous = d.previous.as_ref().map(EvidenceDigest::as_str);
        let valid=sqlx::query_scalar!(r#"SELECT
            (NOT EXISTS(SELECT 1 FROM edgeagent_stream_heads WHERE session_id=$1 AND instrument=$2) AND $3::text IS NULL)
            OR EXISTS(SELECT 1 FROM edgeagent_stream_heads h JOIN edgeagent_stream_publications p USING(publication_id)
                WHERE h.session_id=$1 AND h.instrument=$2 AND frame<$4 AND captured_ms<=$5
                AND ((digest=$3 AND p.delete_ms >= $6) OR (digest IS NULL AND $3 IS NULL))) AS "valid!""#,
            session.data().id.as_str(),d.scope.instrument.as_str(),previous,number(&d.frames[0])?,millis(d.captured_at)?,millis(d.delete_by)?)
            .fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !valid {
            return Err(conflict());
        }
        let bytes = p.bytes()?;
        let descriptor = serde_json::to_string(&p.segment().object()).map_err(|_| invalid())?;
        sqlx::query!("INSERT INTO edgeagent_stream_publications(publication_id,session_id,instrument,preparation,descriptor,delete_ms) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(publication_id) DO NOTHING",p.id().as_str(),session.data().id.as_str(),p.segment().data().scope.instrument.as_str(),bytes,descriptor,millis(p.segment().data().delete_by)?)
            .execute(&mut *tx).await.map_err(|e| {
                if e.as_database_error().and_then(|d|d.code()).as_deref()==Some("23505") {
                    Error::with_source(K::Conflict,e)
                } else { Error::storage(e) }
            })?;
        let same=sqlx::query_scalar!(r#"SELECT COALESCE(session_id=$2 AND preparation=$3 AND NOT retired,FALSE) AS "same!" FROM edgeagent_stream_publications WHERE publication_id=$1"#,p.id().as_str(),session.data().id.as_str(),bytes).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !same {
            return Err(conflict());
        }
        write_guard(&mut tx, session, p, catalogs, cancel).await?;
        tx.commit().await.map_err(Error::storage)
    }
    async fn recover_segment(
        &self,
        session: &StreamingSession,
        id: &EvidenceId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<Option<StreamPublication>> {
        let mut tx = self.ledger.transaction().await?;
        retained_guard(&mut tx, session, catalogs).await?;
        let row=sqlx::query!("SELECT preparation,delete_ms FROM edgeagent_stream_publications WHERE publication_id=$1 AND session_id=$2 AND NOT retired",id.as_str(),session.data().id.as_str()).fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        let value = match row {
            Some(r) => {
                if r.delete_ms <= now(&mut tx).await?.unix_millis() {
                    return Err(denied());
                }
                let p = StreamPublication::decode(r.preparation.as_deref().ok_or_else(invalid)?)?;
                if p.id() != id || p.segment().data().scope.session != session.data().id {
                    return Err(invalid());
                }
                Some(p)
            }
            None => None,
        };
        tx.commit().await.map_err(Error::storage)?;
        Ok(value)
    }
    async fn commit_segment(
        &self,
        session: &StreamingSession,
        p: &StreamPublication,
        catalogs: AuthorityCatalogs<'_>,
        cancel: &Cancellation,
    ) -> Result<StreamCaptureBoundary> {
        let mut tx = self.ledger.transaction().await?;
        write_guard(&mut tx, session, p, catalogs, cancel).await?;
        let bytes = p.bytes()?;
        let r=sqlx::query!(r#"SELECT COALESCE(preparation=$3,FALSE) AS "same!",published,retired,event,history_start,history_retired,reconnect,skipped FROM edgeagent_stream_publications WHERE publication_id=$1 AND session_id=$2 FOR UPDATE"#,p.id().as_str(),session.data().id.as_str(),bytes).fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(conflict)?;
        if !r.same || r.retired {
            return Err(conflict());
        }
        let event = p.envelope().to_json().map_err(|_| invalid())?;
        if r.published {
            if r.event.as_deref() != Some(event.as_slice()) {
                return Err(conflict());
            }
            let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3) AS "intact!""#,p.envelope().source(),p.envelope().id(),event).fetch_one(&mut *tx).await.map_err(Error::storage)?;
            if !intact {
                return Err(invalid());
            }
            tx.commit().await.map_err(Error::storage)?;
            return Ok(StreamCaptureBoundary {
                history_start: r.history_start.ok_or_else(invalid)?,
                history_retired: r.history_retired.ok_or_else(invalid)?,
                reconnect: r.reconnect.ok_or_else(invalid)?,
                skipped: r.skipped.ok_or_else(invalid)?,
            });
        }
        let d = p.segment().data();
        let instrument = d.scope.instrument.as_str();
        let head=sqlx::query!("SELECT digest,frame,generation,epoch,captured_ms FROM edgeagent_stream_heads WHERE session_id=$1 AND instrument=$2 FOR UPDATE",session.data().id.as_str(),instrument).fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        let start = number(&d.frames[0])?;
        let end = number(&d.frames[1])?;
        let generation = number(&d.generation)?;
        let epoch = number(&d.epoch)?;
        let captured = millis(d.captured_at)?;
        if head.as_ref().and_then(|h| h.digest.as_deref())
            != d.previous.as_ref().map(EvidenceDigest::as_str)
            || head
                .as_ref()
                .is_some_and(|h| start <= h.frame || captured < h.captured_ms)
        {
            return Err(conflict());
        }
        let boundary = StreamCaptureBoundary {
            history_start: head.as_ref().is_none_or(|h| h.digest.is_none()),
            history_retired: head.as_ref().is_some_and(|h| h.digest.is_none()),
            reconnect: head
                .as_ref()
                .is_some_and(|h| h.generation != generation || h.epoch != epoch),
            skipped: head.as_ref().map_or(start > 1, |h| start != h.frame + 1),
        };
        PostgresOutbox
            .enqueue(&mut tx, STREAM_PUBLISHED, p.envelope())
            .await
            .map_err(|e| {
                let kind = match e.kind() {
                    edgeagent_outbox_postgres::OutboxErrorKind::Storage => K::Storage,
                    edgeagent_outbox_postgres::OutboxErrorKind::MessageIdentityConflict => {
                        K::Conflict
                    }
                    _ => K::Invalid,
                };
                Error::with_source(kind, e)
            })?;
        sqlx::query!("INSERT INTO edgeagent_stream_heads(session_id,instrument,publication_id,digest,frame,generation,epoch,captured_ms) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(session_id,instrument) DO UPDATE SET publication_id=excluded.publication_id,digest=excluded.digest,frame=excluded.frame,generation=excluded.generation,epoch=excluded.epoch,captured_ms=excluded.captured_ms",session.data().id.as_str(),instrument,p.id().as_str(),p.segment().digest().as_str(),end,generation,epoch,captured).execute(&mut *tx).await.map_err(Error::storage)?;
        sqlx::query!("UPDATE edgeagent_stream_publications SET published=TRUE,event=$2,history_start=$3,reconnect=$4,skipped=$5,history_retired=$6 WHERE publication_id=$1",p.id().as_str(),event,boundary.history_start,boundary.reconnect,boundary.skipped,boundary.history_retired).execute(&mut *tx).await.map_err(Error::storage)?;
        write_guard(&mut tx, session, p, catalogs, cancel).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(boundary)
    }
    async fn published_segment(
        &self,
        session: &StreamingSession,
        id: &EvidenceId,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<StreamSegmentObject> {
        let mut tx = self.ledger.transaction().await?;
        retained_guard(&mut tx, session, catalogs).await?;
        let row=sqlx::query!("SELECT descriptor,delete_ms FROM edgeagent_stream_publications WHERE publication_id=$1 AND session_id=$2 AND published AND NOT retired",id.as_str(),session.data().id.as_str()).fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(denied)?;
        if row.delete_ms <= now(&mut tx).await?.unix_millis() {
            return Err(denied());
        }
        let object = serde_json::from_str(&row.descriptor).map_err(|_| invalid())?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(object)
    }
}
