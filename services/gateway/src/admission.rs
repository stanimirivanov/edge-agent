//! Gateway-owned scoped request and evaluation-command transaction.
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    research::{ResearchRecord, ResearchRequest},
};
use edgeagent_outbox_postgres::{EnqueueDisposition, PostgresOutbox};
use edgeagent_research_application::publication::{self, EVALUATE};
pub use edgeagent_research_application::publication::{
    PublicationError as Error, PublicationErrorKind as Kind,
};
/// Apply explicitly with the outbox migrations in a private Gateway schema.
pub const MIGRATION: &str = include_str!("../migrations/0001_gateway_requests.sql");
/// HTTP acknowledgement; produced by the store only after commit.
#[derive(Debug, Eq, PartialEq, serde::Serialize)]
pub struct Accepted {
    request_id: String,
    scope: String,
    status: &'static str,
}
impl Accepted {
    /// Construct after a successful durable admission (or equivalent test double).
    #[must_use]
    pub fn committed(request: &ResearchRequest) -> Self {
        Self {
            request_id: request.request_id.clone(),
            scope: request.scope.clone(),
            status: "accepted",
        }
    }
}
/// Consumer-owned substitution boundary for HTTP acknowledgement tests.
pub trait AdmissionStore: Send + Sync + 'static {
    /// Operator-configured scope, never taken from client authority claims.
    fn scope(&self) -> &str;
    /// Persist intent and command atomically, or recover the original admission.
    /// # Errors
    /// Invalid input, conflicting scoped identity, or unavailable/corrupt storage.
    fn admit(
        &self,
        request: ResearchRequest,
    ) -> impl std::future::Future<Output = Result<Accepted, Error>> + Send;
}
/// Stable ASCII operator/request scope token.
#[must_use]
pub fn valid_scope(scope: &str) -> bool {
    !scope.is_empty()
        && scope.len() <= 64
        && scope
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
/// Validate v1 transport bounds and the canonical instrument-identity profile.
/// # Errors
/// Rejects a foreign scope, invalid record or noncanonical/zero instrument identity.
pub fn validate(request: &ResearchRequest, scope: &str) -> Result<(), Error> {
    let invalid = || Error::new(Kind::Invalid);
    if request.scope != scope
        || !valid_scope(scope)
        || request.instruments.len() > 64
        || request.instruments.iter().any(|s| {
            s.len() != 32
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || s.bytes().all(|b| b == b'0')
        })
        || request.implementation.len() != 64
        || request.configuration.len() != 64
        || request.implementation.bytes().all(|b| b == b'0')
        || request.configuration.bytes().all(|b| b == b'0')
        || request.request_id.len() > 64
        || request.scope.len() > 64
    {
        return Err(invalid());
    }
    ResearchRecord::Request(request.clone())
        .canonical_bytes()
        .map_err(|_| invalid())?;
    Ok(())
}
/// Pool must be scoped to the Gateway-owned schema; no migration or broker I/O.
pub struct PostgresAdmission {
    pool: sqlx::PgPool,
    scope: String,
}
impl PostgresAdmission {
    /// Bind one local operator identity.
    /// # Errors
    /// Rejects malformed scope configuration.
    pub fn new(pool: sqlx::PgPool, scope: String) -> Result<Self, Error> {
        if !valid_scope(&scope) {
            return Err(Error::new(Kind::Invalid));
        }
        Ok(Self { pool, scope })
    }
}
fn storage(e: sqlx::Error) -> Error {
    Error::with_source(Kind::Storage, e)
}
fn invariant() -> Error {
    Error::new(Kind::Storage)
}
fn command(request: &ResearchRequest, time: &str) -> Result<MessageEnvelope, Error> {
    let record = ResearchRecord::Request(request.clone());
    let key = record.digest().map_err(|_| Error::new(Kind::Invalid))?;
    let partition = format!("research-request/{}/{}", request.scope, request.request_id);
    // Deterministic trace identity with nonzero prefixes, distinct ingress causation.
    let trace = format!("00-1{}-1{}-01", &key[..31], &key[32..47]);
    MessageEnvelope::from_payload(
        MessageMetadata {
            id: key.clone(),
            source: Component::Gateway.source_uri().into(),
            message_type: EVALUATE.message_type().into(),
            subject: partition.clone(),
            time: time.into(),
            data_schema: EVALUATE.data_schema().into(),
            correlation_id: key.clone(),
            causation_id: format!("ingress-{key}"),
            idempotency_key: key,
            partition_key: partition,
            trace_parent: trace,
            trace_state: None,
        },
        &record,
    )
    .map_err(|_| invariant())
}
impl AdmissionStore for PostgresAdmission {
    fn scope(&self) -> &str {
        &self.scope
    }
    async fn admit(&self, request: ResearchRequest) -> Result<Accepted, Error> {
        validate(&request, &self.scope)?;
        let bytes = ResearchRecord::Request(request.clone())
            .canonical_bytes()
            .map_err(|_| Error::new(Kind::Invalid))?;
        let mut tx = self.pool.begin().await.map_err(storage)?;
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
        let time=sqlx::query_scalar!(r#"SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "time!""#).fetch_one(&mut *tx).await.map_err(storage)?;
        let candidate = command(&request, &time)?;
        let envelope = candidate.to_json().map_err(|_| invariant())?;
        let inserted=sqlx::query!("INSERT INTO edgeagent_gateway_requests (caller_scope,request_id,request,command) VALUES ($1,$2,$3,$4) ON CONFLICT (caller_scope,request_id) DO NOTHING",request.scope,request.request_id,bytes,envelope).execute(&mut *tx).await.map_err(storage)?.rows_affected()==1;
        if inserted {
            let disposition = PostgresOutbox
                .enqueue(&mut tx, EVALUATE, &candidate)
                .await
                .map_err(|e| Error::with_source(Kind::Storage, e))?;
            if disposition != EnqueueDisposition::Inserted {
                return Err(invariant());
            }
        }
        // A separate READ COMMITTED statement sees any concurrent winning commit.
        let row=sqlx::query!("SELECT request,command FROM edgeagent_gateway_requests WHERE caller_scope=$1 AND request_id=$2",request.scope,request.request_id).fetch_one(&mut *tx).await.map_err(storage)?;
        if row.request != bytes {
            return Err(Error::new(Kind::Conflict));
        }
        let retained = MessageEnvelope::from_json(&row.command).map_err(|_| invariant())?;
        if publication::evaluation_request(&retained).map_err(|_| invariant())? != request
            || retained.to_json().map_err(|_| invariant())? != row.command
        {
            return Err(invariant());
        }
        let subject = EVALUATE.subject().map_err(|_| invariant())?;
        let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4 AND transport_subject=$5) AS "intact!""#,retained.source(),retained.id(),row.command,EVALUATE.message_type(),subject).fetch_one(&mut *tx).await.map_err(storage)?;
        if !intact {
            return Err(invariant());
        }
        tx.commit().await.map_err(storage)?;
        Ok(Accepted::committed(&request))
    }
}
