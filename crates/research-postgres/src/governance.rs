//! Owner-authorized append-only exact-profile governance. No caller-supplied actor identity.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_contracts::strategy_governance::Profile;
use edgeagent_research_application::publication::{
    PublicationError as Error, PublicationErrorKind as Kind,
};
/// Apply after the completion migration, in the same private schema.
pub const MIGRATION: &str = include_str!("../migrations/0003_strategy_governance.sql");
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
impl PostgresResearchStore {
    /// Append installed (1), approved (2), or terminal retired (3), or recover an exact retry.
    /// Schema ownership of the authenticated PostgreSQL session is local operator authority.
    pub async fn govern(&self, profile: &Profile, revision: i16) -> Result<(), Error> {
        let bytes = profile.bytes().map_err(|_| invalid())?;
        let action = match revision {
            1 => "installed",
            2 => "approved",
            3 => "retired",
            _ => return Err(invalid()),
        };
        let mut tx = transaction(&self.pool).await?;
        let actor = sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(invalid)?;
        // Serialize installation too: the definition may not exist yet.
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            profile.scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        if revision == 1 {
            sqlx::query!("INSERT INTO edgeagent_research_profiles VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",profile.scope,profile.implementation,profile.configuration,bytes).execute(&mut *tx).await.map_err(storage)?;
        }
        let retained=sqlx::query_scalar!("SELECT profile FROM edgeagent_research_profiles WHERE caller_scope=$1 AND implementation=$2 AND configuration=$3 FOR UPDATE",profile.scope,profile.implementation,profile.configuration).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(invalid)?;
        if retained != bytes {
            return Err(Error::new(Kind::Conflict));
        }
        let last=sqlx::query!("SELECT revision,action,actor FROM edgeagent_research_approvals WHERE caller_scope=$1 AND implementation=$2 AND configuration=$3 ORDER BY revision DESC LIMIT 1",profile.scope,profile.implementation,profile.configuration).fetch_optional(&mut *tx).await.map_err(storage)?;
        if let Some(last) = last {
            if last.revision == revision && last.action == action && last.actor == actor {
                tx.commit().await.map_err(storage)?;
                return Ok(());
            }
            if last.revision != revision - 1 {
                return Err(Error::new(Kind::Conflict));
            }
        } else if revision != 1 {
            return Err(invalid());
        }
        sqlx::query!("INSERT INTO edgeagent_research_approvals (caller_scope,implementation,configuration,revision,action,actor) VALUES ($1,$2,$3,$4,$5,$6)",profile.scope,profile.implementation,profile.configuration,revision,action,actor).execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)
    }
}
/// Hold the definition read lock through completion, fencing retirement's write lock.
pub(super) async fn check(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    profile: &Profile,
) -> Result<(), Error> {
    let bytes = profile.bytes().map_err(|_| invalid())?;
    let retained=sqlx::query_scalar!("SELECT profile FROM edgeagent_research_profiles WHERE caller_scope=$1 AND implementation=$2 AND configuration=$3 FOR SHARE",profile.scope,profile.implementation,profile.configuration).fetch_optional(&mut **tx).await.map_err(storage)?.ok_or_else(invalid)?;
    let action=sqlx::query_scalar!("SELECT action FROM edgeagent_research_approvals WHERE caller_scope=$1 AND implementation=$2 AND configuration=$3 ORDER BY revision DESC LIMIT 1",profile.scope,profile.implementation,profile.configuration).fetch_optional(&mut **tx).await.map_err(storage)?;
    if retained != bytes || action.as_deref() != Some("approved") {
        return Err(invalid());
    }
    Ok(())
}
