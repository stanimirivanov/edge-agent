//! Schema-owner append and read operations for an isolated experiment family.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_contracts::research_experiment::{Action, Command};
use edgeagent_research_application::{
    experiment_ledger::Ledger,
    publication::{PublicationError as Error, PublicationErrorKind as Kind},
};

/// Apply after the existing Research migrations in the private Research schema.
pub const MIGRATION: &str = include_str!("../migrations/0006_research_experiment_ledger.sql");

impl PostgresResearchStore {
    /// Apply a canonical command or read one ordered family as canonical JSON lines.
    /// Only an authenticated schema owner may inspect or modify the ledger.
    pub async fn experiment_ledger(&self, command: &Command) -> Result<Vec<u8>, Error> {
        let invalid = || Error::new(Kind::Invalid);
        let bytes = command
            .bytes()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let Command::V1 {
            scope,
            family,
            action,
        } = command;
        let mut tx = transaction(&self.pool).await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#)
            .fetch_optional(&mut *tx).await.map_err(storage)?.ok_or_else(invalid)?;
        // Tokens exclude slashes, so the namespaced scope/family pair is unambiguous.
        let lock = format!("research-experiment/{scope}/{family}");
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            lock
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let rows = sqlx::query!("SELECT ordinal,record FROM edgeagent_research_experiment_ledger WHERE caller_scope=$1 AND family=$2 ORDER BY ordinal LIMIT 67", scope, family)
            .fetch_all(&mut *tx).await.map_err(storage)?;
        let mut ledger = Ledger::default();
        for (ordinal, row) in rows.iter().enumerate() {
            let retained =
                Command::decode(&row.record).map_err(|e| Error::with_source(Kind::Invalid, e))?;
            let Command::V1 {
                scope: retained_scope,
                family: retained_family,
                ..
            } = &retained;
            if usize::try_from(row.ordinal).ok() != Some(ordinal)
                || scope != retained_scope
                || family != retained_family
                || !ledger
                    .apply(&retained)
                    .map_err(|e| Error::with_source(Kind::Invalid, e))?
            {
                return Err(invalid());
            }
        }
        let response = if matches!(action, Action::Read) {
            if rows.is_empty() {
                return Err(invalid());
            }
            rows.into_iter().flat_map(|row| row.record).collect()
        } else {
            let ordinal = i32::try_from(rows.len()).map_err(|_| invalid())?;
            if ledger
                .apply(command)
                .map_err(|e| Error::with_source(Kind::Invalid, e))?
            {
                sqlx::query!("INSERT INTO edgeagent_research_experiment_ledger(caller_scope,family,ordinal,record,actor) VALUES($1,$2,$3,$4,$5)", scope, family, ordinal, bytes, actor)
                    .execute(&mut *tx).await.map_err(storage)?;
            }
            bytes
        };
        tx.commit().await.map_err(storage)?;
        Ok(response)
    }
}
