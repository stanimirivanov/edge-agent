//! Exact-profile owner qualification approval; independent of evidence disclosure rights.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_contracts::explanation_profile::Profile;
use edgeagent_research_application::{
    explanation_qualification::VerifiedReview,
    publication::{PublicationError as Error, PublicationErrorKind as Kind},
};
use sqlx::{Postgres, Transaction};
/// Apply after Research migration 0011 in the private schema.
pub const MIGRATION: &str = include_str!("../migrations/0012_explanation_qualification.sql");
/// Current database observation. Recheck under the dispatch/commit transaction fence.
pub struct Approval {
    pub manifest: String,
    pub helper: String,
    pub suite: String,
    pub preparation: String,
    pub revision: i64,
    pub collection: String,
    pub assessment: String,
}
struct Stored {
    revision: i64,
    command: Vec<u8>,
    collection: String,
    assessment: String,
    approved: bool,
    actor: String,
}
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
fn key(scope: &str, profile: &Profile) -> Result<String, Error> {
    if scope.is_empty()
        || scope.len() > 64
        || !scope
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(invalid());
    }
    Ok(format!(
        "explanation-qualification/{scope}/{}",
        profile.id().map_err(|_| invalid())?
    ))
}
async fn latest(
    tx: &mut Transaction<'_, Postgres>,
    scope: &str,
    profile: &str,
) -> Result<Option<Stored>, Error> {
    sqlx::query_as!(Stored,r#"SELECT revision,command,collection,assessment,approved,actor::text AS "actor!" FROM edgeagent_explanation_qualification WHERE caller_scope=$1 AND profile=$2 ORDER BY revision DESC LIMIT 1"#,scope,profile).fetch_optional(&mut **tx).await.map_err(storage)
}
impl PostgresResearchStore {
    /// Append the next independently rescored review as authenticated schema owner.
    /// Exact latest retries are actor-bound; no historical approval is upgraded.
    pub async fn review_qualification(&self, review: &VerifiedReview) -> Result<(), Error> {
        let command = review.command();
        let profile = command.profile().id().map_err(|_| invalid())?;
        let lock = key(command.scope(), command.profile())?;
        let mut tx = transaction(&self.pool).await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(invalid)?;
        sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended($1,0))", lock)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        let next = match latest(&mut tx, command.scope(), &profile).await? {
            Some(last) => {
                if last.revision == command.revision()
                    && last.command == review.bytes()
                    && last.collection == review.collection()
                    && last.assessment == review.assessment()
                    && last.approved == command.approved()
                    && last.actor == actor
                {
                    return tx.commit().await.map_err(storage);
                }
                last.revision.checked_add(1)
            }
            None => Some(1),
        };
        if next != Some(command.revision()) {
            return Err(Error::new(Kind::Conflict));
        }
        sqlx::query!("INSERT INTO edgeagent_explanation_qualification(caller_scope,profile,revision,command,assessment,collection,approved,actor) VALUES($1,$2,$3,$4,$5,$6,$7,session_user)",command.scope(),profile,command.revision(),review.bytes(),review.assessment(),review.collection(),command.approved()).execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)
    }
    /// Check the latest exact profile under a shared fence held until transaction end.
    /// Use a Research READ COMMITTED transaction. No result grants disclosure rights.
    pub async fn current_qualification_approval(
        tx: &mut Transaction<'_, Postgres>,
        scope: &str,
        profile: &Profile,
    ) -> Result<Option<Approval>, Error> {
        let lock = key(scope, profile)?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))",
            lock
        )
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
        let Some(last) = latest(tx, scope, &profile.id().map_err(|_| invalid())?).await? else {
            return Ok(None);
        };
        let verified = VerifiedReview::decode(&last.command).map_err(|_| invalid())?;
        let c = verified.command();
        if c.scope() != scope
            || c.profile() != profile
            || c.revision() != last.revision
            || c.approved() != last.approved
            || verified.collection() != last.collection
            || verified.assessment() != last.assessment
        {
            return Err(invalid());
        }
        let edgeagent_contracts::explanation_qualification::Command::Approve { evidence, .. } = c
        else {
            return Ok(None);
        };
        Ok(Some(Approval {
            manifest: evidence.collection.manifest.clone(),
            helper: evidence.collection.helper.clone(),
            suite: evidence.collection.suite.clone(),
            preparation: evidence.collection.preparation.clone(),
            revision: last.revision,
            collection: last.collection,
            assessment: last.assessment,
        }))
    }
    /// Fresh observation before an operation; this is not a cacheable permission lease.
    pub async fn qualification_approval(
        &self,
        scope: &str,
        profile: &Profile,
    ) -> Result<Option<Approval>, Error> {
        let mut tx = transaction(&self.pool).await?;
        let observed = Self::current_qualification_approval(&mut tx, scope, profile).await?;
        tx.commit().await.map_err(storage)?;
        Ok(observed)
    }
}
