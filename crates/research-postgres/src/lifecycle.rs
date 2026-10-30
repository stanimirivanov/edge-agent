//! Append-only owner commands. Serialize one scope to fence correction-target races.
use super::{PostgresResearchStore, read, storage, transaction};
use edgeagent_contracts::{
    MessageEnvelope,
    research::{LifecycleChange, ResearchArtifact, ResearchLifecycle, ResearchRecord},
};
use edgeagent_outbox_postgres::{EnqueueDisposition, PostgresOutbox};
use edgeagent_research_application::{
    lifecycle,
    publication::{CommittedOutcome, PublicationError as Error, PublicationErrorKind as Kind},
};
/// Apply explicitly after completion/inbox/outbox migrations.
pub const MIGRATION: &str = include_str!("../migrations/0002_research_lifecycle.sql");
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
async fn history(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: &str,
    id: &str,
) -> Result<Vec<(ResearchLifecycle, MessageEnvelope)>, Error> {
    let rows=sqlx::query!("SELECT revision,record,publication FROM edgeagent_research_lifecycle WHERE caller_scope=$1 AND request_id=$2 ORDER BY revision LIMIT 3",scope,id).fetch_all(&mut **tx).await.map_err(storage)?;
    if rows.len() > 2 {
        return Err(invalid());
    }
    let mut result = Vec::new();
    for row in rows {
        let envelope = MessageEnvelope::from_json(&row.publication)
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let event = lifecycle::decode(&envelope)?;
        if i64::try_from(event.revision).ok() != Some(row.revision)
            || ResearchRecord::Lifecycle(event.clone())
                .canonical_bytes()
                .map_err(|e| Error::with_source(Kind::Invalid, e))?
                != row.record
            || envelope
                .to_json()
                .map_err(|e| Error::with_source(Kind::Invalid, e))?
                != row.publication
        {
            return Err(invalid());
        }
        let subject = lifecycle::CHANGED
            .subject()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4 AND transport_subject=$5) AS "intact!""#,envelope.source(),envelope.id(),row.publication,lifecycle::CHANGED.message_type(),subject).fetch_one(&mut **tx).await.map_err(storage)?;
        if !intact {
            return Err(invalid());
        }
        result.push((event, envelope));
    }
    Ok(result)
}
async fn artifact(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: &str,
    digest: &str,
) -> Result<ResearchArtifact, Error> {
    let row=sqlx::query!("SELECT outcome,publication FROM edgeagent_research_completions WHERE caller_scope=$1 AND encode(sha256(outcome),'hex')=$2",scope,digest).fetch_optional(&mut **tx).await.map_err(storage)?.ok_or_else(invalid)?;
    let receipt = CommittedOutcome::decode(&row.outcome, &row.publication)?;
    let ResearchRecord::Artifact(base) = receipt.record() else {
        return Err(invalid());
    };
    read(tx, &base.request).await?.ok_or_else(invalid)?;
    if base.request.scope != scope {
        return Err(invalid());
    }
    Ok(base.clone())
}
impl PostgresResearchStore {
    /// Execute one independently authorized local owner command, atomically with its event.
    /// Correction targets must already be intact, compatible and still published.
    /// # Errors
    /// Rejects foreign scope, missing base/target, conflicts, illegal revisions or storage failure.
    pub async fn append_lifecycle(
        &self,
        scope: &str,
        event: ResearchLifecycle,
        time: &str,
    ) -> Result<MessageEnvelope, Error> {
        if event.scope != scope {
            return Err(invalid());
        }
        let bytes = ResearchRecord::Lifecycle(event.clone())
            .canonical_bytes()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let mut tx = transaction(&self.pool).await?;
        let lock = format!("research-lifecycle/{scope}");
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            lock
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let base = artifact(&mut tx, scope, &event.artifact).await?;
        if base.request.request_id != event.request_id {
            return Err(invalid());
        }
        let retained = history(&mut tx, scope, &event.request_id).await?;
        let mut events = retained.iter().map(|(e, _)| e.clone()).collect::<Vec<_>>();
        lifecycle::validate(&base, &events)?;
        if let Some((old, envelope)) = retained.iter().find(|(e, _)| e.revision == event.revision) {
            if old != &event {
                return Err(Error::new(Kind::Conflict));
            }
            let envelope = envelope.clone();
            tx.commit().await.map_err(storage)?;
            return Ok(envelope);
        }
        events.push(event.clone());
        lifecycle::validate(&base, &events)?;
        if let LifecycleChange::Corrected { replacement } = &event.change {
            let target = artifact(&mut tx, scope, replacement).await?;
            let target_history = history(&mut tx, scope, &target.request.request_id).await?;
            if !target_history.is_empty()
                || target.request.scope != base.request.scope
                || target.request.request_id == base.request.request_id
                || target.request.decision_at_ms != base.request.decision_at_ms
                || target.request.instruments != base.request.instruments
                || target.request.lookback_sessions != base.request.lookback_sessions
                || target.expires_at_ms <= event.effective_at_ms
            {
                return Err(invalid());
            }
        }
        let envelope = lifecycle::publication(&event, time)?;
        let publication = envelope
            .to_json()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let revision =
            i64::try_from(event.revision).map_err(|e| Error::with_source(Kind::Invalid, e))?;
        sqlx::query!("INSERT INTO edgeagent_research_lifecycle(caller_scope,request_id,revision,record,publication) VALUES($1,$2,$3,$4,$5)",scope,event.request_id,revision,bytes,publication).execute(&mut *tx).await.map_err(storage)?;
        if PostgresOutbox
            .enqueue(&mut tx, lifecycle::CHANGED, &envelope)
            .await
            .map_err(|e| Error::with_source(Kind::Storage, e))?
            != EnqueueDisposition::Inserted
        {
            return Err(invalid());
        }
        tx.commit().await.map_err(storage)?;
        Ok(envelope)
    }
}
