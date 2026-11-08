//! Research-owned disclosure grants. Evidence-owner permission is independent.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_research_application::{
    explanation_context::{Grant, Request},
    publication::{PublicationError as Error, PublicationErrorKind as Kind},
};
use sqlx::{Postgres, Transaction};

/// Apply in the private Research schema after migration 0007.
pub const MIGRATION: &str = include_str!("../migrations/0008_explanation_grants.sql");

/// The exact Research grant revision observed under the scope fence.
pub struct Observation {
    pub revision: i64,
    pub grant: Grant,
}

struct Stored {
    revision: i64,
    valid_from_ms: i64,
    expires_at_ms: i64,
    revoked: bool,
    actor: String,
}

async fn latest(
    tx: &mut Transaction<'_, Postgres>,
    request: &Request,
) -> Result<Option<Stored>, Error> {
    sqlx::query_as!(Stored, "SELECT revision,valid_from_ms,expires_at_ms,revoked,actor FROM edgeagent_explanation_grants WHERE caller_scope=$1 AND artifact=$2 AND decision=$3 AND audience=$4 AND purpose=$5 AND document=$6 ORDER BY revision DESC LIMIT 1",
        request.anchor.scope, request.anchor.artifact, request.anchor.decision,
        request.audience, request.purpose.code(), request.document.as_deref().unwrap_or(""))
        .fetch_optional(&mut **tx).await.map_err(storage)
}

impl PostgresResearchStore {
    /// Append the next owner grant revision. An exact latest retry is idempotent.
    /// Stale and conflicting updates cannot replace the active revision.
    pub async fn set_explanation_grant(&self, grant: &Grant, revision: i64) -> Result<(), Error> {
        let request = &grant.request;
        if !request.is_valid()
            || revision <= 0
            || grant.valid_from_ms < 0
            || grant.expires_at_ms <= grant.valid_from_ms
        {
            return Err(Error::new(Kind::Invalid));
        }
        let mut tx = transaction(&self.pool).await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#)
            .fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(|| Error::new(Kind::Invalid))?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            request.anchor.scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let next = match latest(&mut tx, request).await? {
            Some(last) => {
                if revision == last.revision
                    && last.actor == actor
                    && last.valid_from_ms == grant.valid_from_ms
                    && last.expires_at_ms == grant.expires_at_ms
                    && last.revoked == grant.revoked
                {
                    return tx.commit().await.map_err(storage);
                }
                last.revision.checked_add(1)
            }
            None => Some(1),
        };
        if next != Some(revision) {
            return Err(Error::new(Kind::Conflict));
        }
        sqlx::query!("INSERT INTO edgeagent_explanation_grants(caller_scope,artifact,decision,audience,purpose,document,revision,valid_from_ms,expires_at_ms,revoked,actor) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            request.anchor.scope, request.anchor.artifact, request.anchor.decision,
            request.audience, request.purpose.code(), request.document.as_deref().unwrap_or(""),
            revision, grant.valid_from_ms, grant.expires_at_ms, grant.revoked, actor)
            .execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)
    }

    /// Check the exact current grant under a shared lock held until transaction end.
    /// Use a READ COMMITTED Research transaction and a trusted current clock.
    /// This result alone does not establish evidence-owner permission.
    pub async fn current_explanation_grant(
        tx: &mut Transaction<'_, Postgres>,
        request: &Request,
        now_ms: i64,
    ) -> Result<Option<Observation>, Error> {
        if !request.is_valid() || now_ms < 0 {
            return Err(Error::new(Kind::Invalid));
        }
        sqlx::query!(
            "SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))",
            request.anchor.scope
        )
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
        Ok(latest(tx, request)
            .await?
            .filter(|row| !row.revoked && row.valid_from_ms <= now_ms && now_ms < row.expires_at_ms)
            .map(|row| Observation {
                revision: row.revision,
                grant: Grant {
                    request: request.clone(),
                    valid_from_ms: row.valid_from_ms,
                    expires_at_ms: row.expires_at_ms,
                    revoked: row.revoked,
                },
            }))
    }
}
