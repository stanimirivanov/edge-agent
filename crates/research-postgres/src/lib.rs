//! Research-owned exact completion/inbox/outbox transaction; no external I/O.
#![forbid(unsafe_code)]
pub mod governance;
pub mod lifecycle;
use edgeagent_contracts::{
    MessageEnvelope,
    research::{ResearchRecord, ResearchRequest},
};
use edgeagent_inbox_postgres::{DeliveryDisposition, InboxErrorKind, PostgresInbox};
use edgeagent_outbox_postgres::{EnqueueDisposition, OutboxErrorKind, PostgresOutbox};
use edgeagent_research_application::publication::{
    self, CommittedOutcome, PreparedCompletion, PublicationError as Error,
    PublicationErrorKind as Kind, ResearchPublication,
};
/// Additive service-owned migration; apply inbox/outbox migrations in the same private schema.
pub const MIGRATION: &str = include_str!("../migrations/0001_research_completions.sql");
/// Caller supplies a pool scoped to the Research-owned schema.
pub struct PostgresResearchStore {
    pool: sqlx::PgPool,
    governed: Option<edgeagent_contracts::strategy_governance::Profile>,
}
impl PostgresResearchStore {
    /// Bind the service-owned database pool; does not migrate or access the database.
    #[must_use]
    pub const fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            governed: None,
        }
    }
    /// Explicit staged governed path; legacy callers remain unchanged until M05-PR12.
    pub fn governed(
        pool: sqlx::PgPool,
        profile: edgeagent_contracts::strategy_governance::Profile,
    ) -> Self {
        Self {
            pool,
            governed: Some(profile),
        }
    }
    async fn check_governance(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        request: &ResearchRequest,
    ) -> Result<(), Error> {
        if let Some(p) = &self.governed {
            if request.scope != p.scope
                || request.implementation != p.implementation
                || request.configuration != p.configuration
                || request.lookback_sessions != p.lookback_sessions
            {
                return Err(Error::new(Kind::Invalid));
            }
            governance::check(tx, p).await?;
        }
        Ok(())
    }
    /// Load an intact original completion/input for replay in a read-only transaction.
    /// Unlike recovery this never records a new inbox delivery.
    /// # Errors
    /// Rejects changed intent, nonoriginal input or missing/corrupt retained integrity.
    pub async fn retained_for_replay(
        &self,
        delivery: &MessageEnvelope,
    ) -> Result<Option<CommittedOutcome>, Error> {
        let request = publication::evaluation_request(delivery)?;
        let mut tx = transaction(&self.pool).await?;
        sqlx::query!("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        let receipt = read(&mut tx, &request).await?;
        if let Some(receipt) = &receipt {
            let bytes = delivery
                .to_json()
                .map_err(|e| Error::with_source(Kind::Invalid, e))?;
            let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_inbox WHERE consumer_name='research-completion' AND message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4) AS "intact!""#,delivery.source(),delivery.id(),bytes,delivery.message_type()).fetch_one(&mut *tx).await.map_err(storage)?;
            if !intact
                || receipt.publication().extension("causationid") != Some(delivery.id())
                || receipt.publication().extension("correlationid")
                    != delivery.extension("correlationid")
            {
                return Err(Error::new(Kind::Invalid));
            }
        }
        tx.commit().await.map_err(storage)?;
        Ok(receipt)
    }
}
fn storage(error: sqlx::Error) -> Error {
    Error::with_source(Kind::Storage, error)
}
async fn read(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &ResearchRequest,
) -> Result<Option<CommittedOutcome>, Error> {
    let row=sqlx::query!("SELECT request,outcome,publication FROM edgeagent_research_completions WHERE caller_scope=$1 AND request_id=$2",request.scope,request.request_id).fetch_optional(&mut **tx).await.map_err(storage)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.request
        != ResearchRecord::Request(request.clone())
            .canonical_bytes()
            .map_err(|_| Error::new(Kind::Invalid))?
    {
        return Err(Error::new(Kind::Conflict));
    }
    let receipt = CommittedOutcome::decode(&row.outcome, &row.publication)?;
    let retained = match receipt.record() {
        ResearchRecord::Artifact(a) => &a.request,
        ResearchRecord::Abstention(a) => &a.request,
        _ => return Err(Error::new(Kind::Invalid)),
    };
    if retained != request {
        return Err(Error::new(Kind::Invalid));
    }
    let definition = publication::registry()?
        .resolve(receipt.publication().message_type())
        .copied()
        .ok_or_else(|| Error::new(Kind::Invalid))?;
    let subject = definition
        .subject()
        .map_err(|_| Error::new(Kind::Invalid))?;
    let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4 AND transport_subject=$5) AS "intact!""#,receipt.publication().source(),receipt.publication().id(),row.publication,definition.message_type(),subject).fetch_one(&mut **tx).await.map_err(storage)?;
    if !intact {
        return Err(Error::new(Kind::Invalid));
    }
    Ok(Some(receipt))
}
async fn transaction(pool: &sqlx::PgPool) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, Error> {
    let mut tx = pool.begin().await.map_err(storage)?;
    sqlx::query!("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
    sqlx::query!("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
    sqlx::query!("SET LOCAL statement_timeout = '15s'")
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
    Ok(tx)
}
async fn inbox(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    delivery: &MessageEnvelope,
) -> Result<DeliveryDisposition, Error> {
    PostgresInbox
        .record_delivery(
            tx,
            "research-completion",
            &publication::registry()?,
            delivery,
        )
        .await
        .map_err(|e| {
            let kind = if e.kind() == InboxErrorKind::MessageIdentityConflict {
                Kind::Conflict
            } else if e.kind() == InboxErrorKind::Storage {
                Kind::Storage
            } else {
                Kind::Invalid
            };
            Error::with_source(kind, e)
        })
}
impl ResearchPublication for PostgresResearchStore {
    async fn recover(&self, delivery: &MessageEnvelope) -> Result<Option<CommittedOutcome>, Error> {
        let request = publication::evaluation_request(delivery)?;
        let mut tx = transaction(&self.pool).await?;
        let receipt = read(&mut tx, &request).await?;
        if receipt.is_none() {
            self.check_governance(&mut tx, &request).await?;
        }
        if receipt.is_some() {
            inbox(&mut tx, delivery).await?;
        }
        tx.commit().await.map_err(storage)?;
        Ok(receipt)
    }
    async fn commit(&self, p: &PreparedCompletion) -> Result<CommittedOutcome, Error> {
        let mut tx = transaction(&self.pool).await?;
        if read(&mut tx, p.request()).await?.is_none() {
            self.check_governance(&mut tx, p.request()).await?;
        }
        let disposition = inbox(&mut tx, p.delivery()).await?;
        let publication = p
            .publication()
            .to_json()
            .map_err(|_| Error::new(Kind::Invalid))?;
        let inserted=sqlx::query!("INSERT INTO edgeagent_research_completions (caller_scope,request_id,request,outcome,publication) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (caller_scope,request_id) DO NOTHING",p.request().scope,p.request().request_id,p.request_bytes(),p.outcome_bytes(),publication).execute(&mut *tx).await.map_err(storage)?.rows_affected()==1;
        if inserted {
            if disposition == DeliveryDisposition::Duplicate {
                return Err(Error::new(Kind::Invalid));
            }
            let enqueued = PostgresOutbox
                .enqueue(&mut tx, p.definition(), p.publication())
                .await
                .map_err(|e| {
                    let kind = if e.kind() == OutboxErrorKind::MessageIdentityConflict {
                        Kind::Conflict
                    } else {
                        Kind::Storage
                    };
                    Error::with_source(kind, e)
                })?;
            if enqueued != EnqueueDisposition::Inserted {
                return Err(Error::new(Kind::Invalid));
            }
        }
        // Separate READ COMMITTED statement sees a concurrent winner's immutable bytes.
        let receipt = read(&mut tx, p.request())
            .await?
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        tx.commit().await.map_err(storage)?;
        Ok(receipt)
    }
}
