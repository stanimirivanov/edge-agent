//! Append-only local operator controls, serialized with fresh completion by scope advisory locks.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_contracts::research_controls::{Command, Target};
use edgeagent_research_application::publication::{
    PublicationError as Error, PublicationErrorKind as Kind,
};
/// Apply after validated decision migration 0004 in the private Research schema.
pub const MIGRATION: &str = include_str!("../migrations/0005_research_controls.sql");
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
/// Zero identifies the explicit enabled default, never a fabricated audit action.
pub(super) struct Snapshot {
    pub scope: i64,
    pub implementation: i64,
}
impl PostgresResearchStore {
    /// Append a revision-checked switch action with authenticated schema-owner actor/time.
    /// Exact latest retries recover; stale/conflicting commands cannot reset a later switch.
    pub async fn control(&self, command: &Command) -> Result<(), Error> {
        command
            .bytes()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let Command::V1 {
            scope,
            target,
            revision,
            enabled,
            reason_code,
        } = command;
        let implementation = match target {
            Target::Scope => "",
            Target::Implementation { digest } => digest.as_str(),
        };
        let revision =
            i64::try_from(*revision).map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let mut tx = transaction(&self.pool).await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(invalid)?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let last=sqlx::query!("SELECT revision,enabled,reason_code,actor FROM edgeagent_research_controls WHERE caller_scope=$1 AND implementation=$2 ORDER BY revision DESC LIMIT 1",scope,implementation).fetch_optional(&mut *tx).await.map_err(storage)?;
        if let Some(last) = last {
            if revision == last.revision
                && *enabled == last.enabled
                && *reason_code == last.reason_code
                && actor == last.actor
            {
                tx.commit().await.map_err(storage)?;
                return Ok(());
            }
            if last.revision.checked_add(1) != Some(revision) {
                return Err(Error::new(Kind::Conflict));
            }
        } else if revision != 1 {
            return Err(Error::new(Kind::Conflict));
        }
        sqlx::query!("INSERT INTO edgeagent_research_controls(caller_scope,implementation,revision,enabled,reason_code,actor) VALUES($1,$2,$3,$4,$5,$6)",scope,implementation,revision,enabled,reason_code,actor).execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)
    }
}
/// Caller already holds governance's shared scope lock until transaction completion.
pub(super) async fn check(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: &str,
    implementation: &str,
) -> Result<Snapshot, Error> {
    let rows=sqlx::query!("SELECT DISTINCT ON (implementation) implementation,revision,enabled FROM edgeagent_research_controls WHERE caller_scope=$1 AND implementation IN ('',$2) ORDER BY implementation,revision DESC",scope,implementation).fetch_all(&mut **tx).await.map_err(storage)?;
    let mut snapshot = Snapshot {
        scope: 0,
        implementation: 0,
    };
    for row in rows {
        if !row.enabled {
            return Err(Error::new(Kind::Disabled));
        }
        if row.implementation.is_empty() {
            snapshot.scope = row.revision;
        } else {
            snapshot.implementation = row.revision;
        }
    }
    Ok(snapshot)
}
/// Historical receipts validate retained enabled actions without consulting current control state.
pub(super) async fn historical(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: &str,
    implementation: &str,
    snapshot: Snapshot,
) -> Result<(), Error> {
    if snapshot.scope < 0 || snapshot.implementation < 0 {
        return Err(invalid());
    }
    let count=sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM edgeagent_research_controls WHERE caller_scope=$1 AND enabled AND ((implementation='' AND revision=$3) OR (implementation=$2 AND revision=$4))"#,scope,implementation,snapshot.scope,snapshot.implementation).fetch_one(&mut **tx).await.map_err(storage)?;
    if count != i64::from(snapshot.scope > 0) + i64::from(snapshot.implementation > 0) {
        return Err(invalid());
    }
    Ok(())
}
