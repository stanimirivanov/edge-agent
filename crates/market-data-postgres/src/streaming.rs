//! Owner-installed finite stream admissions and serialized durable connection slots.
use crate::{PostgresAcquisitionLedger, qualification};
use edgeagent_contracts::evidence::EvidenceId;
use edgeagent_market_data_application::{
    AuthorityCatalogs, BudgetError as Error, BudgetErrorKind as K, Cancellation, ProviderAccountId,
    StreamingAccountPolicy, StreamingArrival, StreamingConnection, StreamingFrame, StreamingLedger,
    StreamingSession, StreamingUsage,
};
use edgeagent_market_data_domain::UtcInstant;
use sqlx::{PgPool, Postgres, Transaction};
mod worker;
type Result<T> = std::result::Result<T, Error>;
fn invalid() -> Error {
    Error::new(K::Invalid)
}
fn denied() -> Error {
    Error::new(K::Denied)
}
fn conflict() -> Error {
    Error::new(K::Conflict)
}
fn key(account: ProviderAccountId) -> String {
    format!("{:032x}", account.get())
}

/// Adapter over the existing bounded transaction machinery; no external I/O or implicit retries.
#[derive(Clone, Debug)]
pub struct PostgresStreamingLedger {
    ledger: PostgresAcquisitionLedger,
}
/// Owner-private observation, not a permission to dial or a physical-quiescence assertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingStatus {
    /// Monotonic session counters.
    pub usage: StreamingUsage,
    /// Last allocated generation, including an already released generation.
    pub generation: u64,
    /// A connection slot remains unresolved, even after deadline/cancellation.
    pub unresolved: bool,
}
#[derive(sqlx::FromRow)]
struct State {
    starts: i64,
    frames: i64,
    bytes: i64,
    last_frame_bytes: Option<i64>,
    stopped: bool,
    generation: i64,
    active: bool,
}
impl State {
    fn status(&self) -> Result<StreamingStatus> {
        Ok(StreamingStatus {
            usage: StreamingUsage {
                starts: u32::try_from(self.starts).map_err(|_| invalid())?,
                frames: u64::try_from(self.frames).map_err(|_| invalid())?,
                bytes: u64::try_from(self.bytes).map_err(|_| invalid())?,
                last_frame_bytes: self
                    .last_frame_bytes
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| invalid())?,
                stopped: self.stopped,
            },
            generation: u64::try_from(self.generation).map_err(|_| invalid())?,
            unresolved: self.active,
        })
    }
}
#[derive(sqlx::FromRow)]
struct Account {
    connections: i32,
    start_limit: i64,
    session_ms: i64,
    spacing_ms: i64,
    expires_ms: i64,
    charged_starts: i64,
    generation: i64,
    last_start_ms: Option<i64>,
    withdrawn: bool,
}
async fn lock_account(tx: &mut Transaction<'_, Postgres>, account: &str) -> Result<Account> {
    sqlx::query_as!(Account,"SELECT connections,start_limit,session_ms,spacing_ms,expires_ms,charged_starts,generation,last_start_ms,withdrawn FROM edgeagent_stream_accounts WHERE account_id=$1 FOR UPDATE",account)
        .fetch_optional(&mut **tx).await.map_err(Error::storage)?.ok_or_else(||Error::new(K::NotFound))
}
async fn state(tx: &mut Transaction<'_, Postgres>, id: &EvidenceId) -> Result<State> {
    sqlx::query_as!(State,"SELECT starts,frames,bytes,last_frame_bytes,stopped,generation,active FROM edgeagent_stream_sessions WHERE session_id=$1 FOR UPDATE",id.as_str())
        .fetch_optional(&mut **tx).await.map_err(Error::storage)?.ok_or_else(||Error::new(K::NotFound))
}
async fn now(tx: &mut Transaction<'_, Postgres>) -> Result<UtcInstant> {
    let ms = sqlx::query_scalar!(
        r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    UtcInstant::from_unix_millis(ms).map_err(|_| invalid())
}
async fn worker(tx: &mut Transaction<'_, Postgres>, session: &StreamingSession) -> Result<()> {
    let allowed = sqlx::query_scalar!(r#"SELECT edgeagent_stream_worker_allowed() AS "allowed!""#)
        .fetch_one(&mut **tx)
        .await
        .map_err(Error::storage)?;
    if !allowed {
        return Err(denied());
    }
    let row=sqlx::query!(r#"SELECT account_id,admission=$2 AS "same!",withdrawn,independent FROM edgeagent_lock_stream_admission($1)"#,session.data().id.as_str(),session.bytes())
        .fetch_optional(&mut **tx).await.map_err(Error::storage)?.ok_or_else(denied)?;
    if row.account_id.as_deref() != Some(key(session.account()).as_str())
        || !row.same
        || row.withdrawn != Some(false)
        || row.independent != Some(true)
    {
        return Err(denied());
    }
    Ok(())
}
impl PostgresStreamingLedger {
    /// Bind an independently provisioned private schema and bounded pool; performs no I/O.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self {
            ledger: PostgresAcquisitionLedger::new(pool),
        }
    }
    /// Install an exclusive owner-reviewed stream allocation and known opening debit.
    /// Owner asserts external users are quiescent; no provider plan or reset is inferred.
    /// # Errors
    /// Denies non-owner sessions, expired policy, changed retries or invalid opening usage.
    pub async fn configure_account(
        &self,
        account: ProviderAccountId,
        policy: StreamingAccountPolicy,
        opening_starts: u32,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        if policy.expires_at <= now(&mut tx).await? || opening_starts > policy.starts.get() {
            return Err(invalid());
        }
        let account = key(account);
        let revision = i64::from(policy.revision.get());
        let connections = i32::from(policy.connections.get());
        let limit = i64::from(policy.starts.get());
        let session_ms = i64::from(policy.session_ms.get());
        let spacing = i64::from(policy.spacing_ms.get());
        let expiry = policy.expires_at.unix_millis();
        let opening = i64::from(opening_starts);
        sqlx::query!("INSERT INTO edgeagent_stream_accounts(account_id,revision,connections,start_limit,session_ms,spacing_ms,expires_ms,opening_starts,charged_starts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$8) ON CONFLICT DO NOTHING",account,revision,connections,limit,session_ms,spacing,expiry,opening)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        let same=sqlx::query_scalar!(r#"SELECT revision=$2 AND connections=$3 AND start_limit=$4 AND session_ms=$5 AND spacing_ms=$6 AND expires_ms=$7 AND opening_starts=$8 AND NOT withdrawn AS "same!" FROM edgeagent_stream_accounts WHERE account_id=$1 FOR UPDATE"#,account,revision,connections,limit,session_ms,spacing,expiry,opening)
            .fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !same {
            return Err(conflict());
        }
        tx.commit().await.map_err(Error::storage)
    }
    /// Install exact immutable inputs using the independent owner, never a worker callback.
    /// # Errors
    /// Denies stale authority, changed admission/policy or insufficient account lifetime.
    pub async fn admit(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
    ) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let account = key(session.account());
        let policy = lock_account(&mut tx, &account).await?;
        validate_policy(&policy, session, now(&mut tx).await?)?;
        session.recheck(catalogs, now(&mut tx).await?, &Cancellation::default())?;
        sqlx::query!("INSERT INTO edgeagent_stream_admissions(session_id,account_id,admission) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",session.data().id.as_str(),account,session.bytes())
            .execute(&mut *tx).await.map_err(Error::storage)?;
        let same=sqlx::query_scalar!(r#"SELECT account_id=$2 AND admission=$3 AND NOT withdrawn AS "same!" FROM edgeagent_stream_admissions WHERE session_id=$1 FOR SHARE"#,session.data().id.as_str(),account,session.bytes())
            .fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !same {
            return Err(conflict());
        }
        sqlx::query!(
            "INSERT INTO edgeagent_stream_sessions(session_id) VALUES($1) ON CONFLICT DO NOTHING",
            session.data().id.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Permanently stop one admission without freeing slots or refunding any usage.
    /// # Errors
    /// Denies non-owner sessions or an unknown/mismatched account and session.
    pub async fn cancel(&self, account: ProviderAccountId, id: &EvidenceId) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        lock_account(&mut tx, &key(account)).await?;
        let result=sqlx::query!("UPDATE edgeagent_stream_admissions SET withdrawn=TRUE WHERE account_id=$1 AND session_id=$2",key(account),id.as_str()).execute(&mut *tx).await.map_err(Error::storage)?;
        if result.rows_affected() != 1 {
            return Err(conflict());
        }
        sqlx::query!(
            "UPDATE edgeagent_stream_sessions SET stopped=TRUE WHERE session_id=$1",
            id.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Stop every admission under an account; withdrawal cannot reset or renew its allocation.
    /// # Errors
    /// Denies non-owner sessions and unknown accounts.
    pub async fn withdraw_account(&self, account: ProviderAccountId) -> Result<()> {
        let mut tx = self.ledger.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let account = key(account);
        lock_account(&mut tx, &account).await?;
        sqlx::query!(
            "UPDATE edgeagent_stream_accounts SET withdrawn=TRUE WHERE account_id=$1",
            account
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Independently owner-confirm physical connection/provider quiescence for exactly one
    /// generation. Expiry, crash, cancellation or missing process cannot supply this fact.
    /// # Errors
    /// Denies non-owner sessions, mismatched/stale generations and invalid identities.
    pub async fn confirm_quiescent(
        &self,
        account: ProviderAccountId,
        id: &EvidenceId,
        generation: u64,
    ) -> Result<()> {
        let generation = i64::try_from(generation)
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        let mut tx = self.ledger.transaction().await?;
        qualification::require_owner(&mut tx).await?;
        let account = key(account);
        lock_account(&mut tx, &account).await?;
        let result=sqlx::query!("UPDATE edgeagent_stream_sessions s SET active=FALSE FROM edgeagent_stream_admissions a WHERE s.session_id=a.session_id AND a.account_id=$1 AND s.session_id=$2 AND s.generation=$3",account,id.as_str(),generation).execute(&mut *tx).await.map_err(Error::storage)?;
        if result.rows_affected() != 1 {
            return Err(conflict());
        }
        tx.commit().await.map_err(Error::storage)
    }
    /// Observe durable counters and uncertainty, including expired/cancelled sessions.
    /// # Errors
    /// Rejects unknown sessions, invalid private counters and storage failures.
    pub async fn status(&self, id: &EvidenceId) -> Result<StreamingStatus> {
        let mut tx = self.ledger.transaction().await?;
        let result = state(&mut tx, id).await?.status()?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(result)
    }
}
fn validate_policy(policy: &Account, session: &StreamingSession, at: UtcInstant) -> Result<()> {
    let window = session.data().window.bounds();
    if policy.withdrawn
        || at.unix_millis() >= policy.expires_ms
        || window.1.unix_millis() > policy.expires_ms
        || window.1.unix_millis() - window.0.unix_millis() > policy.session_ms
    {
        return Err(denied());
    }
    Ok(())
}
