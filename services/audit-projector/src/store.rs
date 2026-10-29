//! Atomic projector checkpoint/inbox/progress; only this service's private tables.
use crate::error::{Error, Kind};
use edgeagent_audit_domain::{
    ReplayError, ReplayOutcome,
    research::{ResearchProjection, ResearchStatus},
};
use edgeagent_contracts::{
    MessageEnvelope,
    research::{ResearchRecord, ResearchRequest},
};
use edgeagent_inbox_postgres::{DeliveryDisposition, InboxErrorKind, PostgresInbox};
use edgeagent_research_application::publication::{self, CommittedOutcome};
/// Apply explicitly with all inbox migrations in a private projector schema.
pub const MIGRATION: &str = include_str!("../migrations/0001_research_projection.sql");
/// Internal query snapshot. Revision is local view revision, not a stream watermark.
#[derive(serde::Serialize)]
pub struct View {
    /// Configured caller scope.
    pub scope: String,
    /// Immutable scoped request ID.
    pub request_id: String,
    /// Local pending/terminal projection revision.
    pub projection_revision: u64,
    /// pending/artifact/abstained/gap observation.
    pub status: &'static str,
    /// First successfully applied delivery key; opaque, not globally ordered.
    pub applied_delivery_key: Option<String>,
    #[serde(skip)]
    outcome: Option<Vec<u8>>,
}
impl View {
    /// Exact canonical terminal bytes; None when pending or gapped.
    pub fn outcome_bytes(&self) -> Option<&[u8]> {
        self.outcome.as_deref()
    }
}
/// One local operator scope bound to a projector-owned database pool.
pub struct ProjectionStore {
    pool: sqlx::PgPool,
    scope: String,
}
/// Strict scoped identifier shared by config and bounded query paths.
pub fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn request(record: &ResearchRecord) -> Result<&ResearchRequest, Error> {
    match record {
        ResearchRecord::Request(r) => Ok(r),
        ResearchRecord::Artifact(a) => Ok(&a.request),
        ResearchRecord::Abstention(a) => Ok(&a.request),
        _ => Err(Error::new(Kind::Invalid)),
    }
}
/// Validate the exact Research owner terminal publication before database work.
pub fn terminal(envelope: &MessageEnvelope) -> Result<ResearchRecord, Error> {
    let record: ResearchRecord = envelope
        .payload()
        .map_err(|e| Error::caused(Kind::Invalid, e))?;
    let bytes = record
        .canonical_bytes()
        .map_err(|e| Error::caused(Kind::Invalid, e))?;
    let envelope_bytes = envelope
        .to_json()
        .map_err(|e| Error::caused(Kind::Invalid, e))?;
    CommittedOutcome::decode(&bytes, &envelope_bytes)
        .map_err(|e| Error::caused(Kind::Invalid, e))?;
    Ok(record)
}
fn restore(
    scope: &str,
    id: &str,
    bytes: &[u8],
    checkpoint: &[u8],
) -> Result<ResearchProjection, Error> {
    let ResearchRecord::Request(request) =
        ResearchRecord::decode(bytes).map_err(|e| Error::caused(Kind::Corrupt, e))?
    else {
        return Err(Error::new(Kind::Corrupt));
    };
    if request.scope != scope || request.request_id != id {
        return Err(Error::new(Kind::Corrupt));
    }
    ResearchProjection::restore(request, checkpoint).map_err(|e| Error::caused(Kind::Corrupt, e))
}
impl ProjectionStore {
    /// Bind an explicitly prepared private pool; no schema changes or connections occur.
    pub fn new(pool: sqlx::PgPool, scope: String) -> Result<Self, Error> {
        if !token(&scope) {
            return Err(Error::new(Kind::Scope));
        }
        Ok(Self { pool, scope })
    }
    /// Trusted fixed local scope, never inferred from a query/body authority claim.
    pub fn scope(&self) -> &str {
        &self.scope
    }
    async fn tx(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query!("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query!("SET LOCAL statement_timeout = '15s'")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
    async fn seed(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        view: &ResearchProjection,
    ) -> Result<(), Error> {
        let request = view.request();
        let bytes = ResearchRecord::Request(request.clone())
            .canonical_bytes()
            .map_err(|e| Error::caused(Kind::Invalid, e))?;
        let checkpoint = view.checkpoint()?;
        sqlx::query!("INSERT INTO edgeagent_research_views(caller_scope,request_id,request,checkpoint) VALUES($1,$2,$3,$4) ON CONFLICT(caller_scope,request_id) DO NOTHING",request.scope,request.request_id,bytes,checkpoint).execute(&mut **tx).await?;
        Ok(())
    }
    /// Track an independently trusted request as pending; does not assert owner acceptance.
    pub async fn track(&self, request: ResearchRequest) -> Result<(), Error> {
        let view = ResearchProjection::new(request)?;
        let req = view.request();
        if req.scope != self.scope {
            return Err(Error::new(Kind::Scope));
        }
        let mut tx = self.tx().await?;
        Self::seed(&mut tx, &view).await?;
        let row=sqlx::query!("SELECT request,checkpoint FROM edgeagent_research_views WHERE caller_scope=$1 AND request_id=$2 FOR UPDATE",req.scope,req.request_id).fetch_one(&mut *tx).await?;
        let retained = restore(&req.scope, &req.request_id, &row.request, &row.checkpoint)?;
        if retained.request() != req {
            return Err(Error::new(Kind::Conflict));
        }
        tx.commit().await?;
        Ok(())
    }
    async fn gap(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        scope: &str,
        id: &str,
        bytes: &[u8],
        key: &str,
    ) -> Result<(), Error> {
        sqlx::query!("UPDATE edgeagent_research_views SET gap_envelope=COALESCE(gap_envelope,$3),gap_delivery_key=COALESCE(gap_delivery_key,$4) WHERE caller_scope=$1 AND request_id=$2",scope,id,bytes,key).execute(&mut **tx).await?;
        Ok(())
    }
    /// Commit checkpoint, inbox and applied-delivery marker atomically. A committed gap
    /// retains unresolved input without advancing successful progress or consuming it.
    pub async fn project(
        &self,
        envelope: &MessageEnvelope,
        key: &str,
    ) -> Result<ReplayOutcome, Error> {
        edgeagent_messaging::DeliveryMessageKey::new(key)
            .map_err(|e| Error::caused(Kind::Invalid, e))?;
        let record = terminal(envelope)?;
        let req = request(&record)?.clone();
        if req.scope != self.scope {
            return Err(Error::new(Kind::Scope));
        }
        let bytes = envelope
            .to_json()
            .map_err(|e| Error::caused(Kind::Invalid, e))?;
        let initial = ResearchProjection::new(req.clone())?;
        let mut tx = self.tx().await?;
        Self::seed(&mut tx, &initial).await?;
        let row=sqlx::query!("SELECT request,checkpoint,applied_delivery_key,gap_envelope FROM edgeagent_research_views WHERE caller_scope=$1 AND request_id=$2 FOR UPDATE",req.scope,req.request_id).fetch_one(&mut *tx).await?;
        let mut view = restore(&req.scope, &req.request_id, &row.request, &row.checkpoint)?;
        if row.gap_envelope.is_some() {
            tx.commit().await?;
            return Err(Error::new(Kind::Gap));
        }
        let was_pending = view.revision() == 0;
        if was_pending != row.applied_delivery_key.is_none() {
            return Err(Error::new(Kind::Corrupt));
        }
        let applied = match view.apply(record) {
            Ok(value) => value,
            Err(error @ (ReplayError::Conflict | ReplayError::Gap)) => {
                Self::gap(&mut tx, &req.scope, &req.request_id, &bytes, key).await?;
                tx.commit().await?;
                return Err(Error::caused(Kind::Gap, error));
            }
            Err(error) => return Err(error.into()),
        };
        let disposition = match PostgresInbox
            .record_delivery(
                &mut tx,
                "research-projector",
                &publication::registry().map_err(|e| Error::caused(Kind::Invalid, e))?,
                envelope,
            )
            .await
        {
            Ok(value) => value,
            Err(error) if error.kind() == InboxErrorKind::MessageIdentityConflict => {
                tx.rollback().await?;
                let mut diagnostic = self.tx().await?;
                Self::gap(&mut diagnostic, &req.scope, &req.request_id, &bytes, key).await?;
                diagnostic.commit().await?;
                return Err(Error::caused(Kind::Gap, error));
            }
            Err(error) => return Err(Error::caused(Kind::Storage, error)),
        };
        if disposition == DeliveryDisposition::FirstDelivery {
            let checkpoint = view.checkpoint()?;
            sqlx::query!("UPDATE edgeagent_research_views SET checkpoint=$3,applied_delivery_key=$4 WHERE caller_scope=$1 AND request_id=$2",req.scope,req.request_id,checkpoint,key).execute(&mut *tx).await?;
        } else if was_pending || row.applied_delivery_key.is_none() {
            return Err(Error::new(Kind::Corrupt));
        }
        tx.commit().await?;
        Ok(applied)
    }
    /// Read one internally scoped snapshot; None means not observed, never rejection.
    pub async fn get(&self, scope: &str, id: &str) -> Result<Option<View>, Error> {
        if scope != self.scope {
            return Err(Error::new(Kind::Scope));
        }
        if !token(id) {
            return Err(Error::new(Kind::Invalid));
        }
        let row=sqlx::query!("SELECT request,checkpoint,applied_delivery_key,gap_envelope FROM edgeagent_research_views WHERE caller_scope=$1 AND request_id=$2",scope,id).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let view = restore(scope, id, &row.request, &row.checkpoint)?;
        if (view.revision() == 0) != row.applied_delivery_key.is_none() {
            return Err(Error::new(Kind::Corrupt));
        }
        if let Some(key) = &row.applied_delivery_key {
            edgeagent_messaging::DeliveryMessageKey::new(key)
                .map_err(|e| Error::caused(Kind::Corrupt, e))?;
        }
        if let Some(outcome) = view.outcome_bytes() {
            let id = ResearchRecord::decode(outcome)
                .map_err(|e| Error::caused(Kind::Corrupt, e))?
                .digest()
                .map_err(|e| Error::caused(Kind::Corrupt, e))?;
            let source = edgeagent_contracts::Component::Research.source_uri();
            let proof=sqlx::query!("SELECT envelope,message_type FROM edgeagent_message_inbox WHERE consumer_name='research-projector' AND message_source=$1 AND message_id=$2",source,id).fetch_optional(&self.pool).await?.ok_or_else(||Error::new(Kind::Corrupt))?;
            let envelope = MessageEnvelope::from_json(&proof.envelope)
                .map_err(|e| Error::caused(Kind::Corrupt, e))?;
            if proof.message_type != envelope.message_type()
                || terminal(&envelope)
                    .map_err(|e| Error::caused(Kind::Corrupt, e))?
                    .canonical_bytes()
                    .map_err(|e| Error::caused(Kind::Corrupt, e))?
                    != outcome
            {
                return Err(Error::new(Kind::Corrupt));
            }
        }
        let gap = row.gap_envelope.is_some();
        Ok(Some(View {
            scope: scope.into(),
            request_id: id.into(),
            projection_revision: view.revision(),
            status: if gap {
                "gap"
            } else {
                match view.status() {
                    ResearchStatus::Pending => "pending",
                    ResearchStatus::Artifact => "artifact",
                    ResearchStatus::Abstained => "abstained",
                }
            },
            applied_delivery_key: row.applied_delivery_key,
            outcome: if gap {
                None
            } else {
                view.outcome_bytes().map(<[u8]>::to_vec)
            },
        }))
    }
}
