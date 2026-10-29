#![allow(
    clippy::disallowed_methods,
    reason = "Isolated owned schema and fault injection."
)]
use super::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;
pub struct Database {
    pub pool: PgPool,
    pub schema: String,
    pub key: i64,
    root: PgPool,
}
impl Database {
    pub async fn new() -> Result<Self> {
        let url = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        edgeagent_audit_projector::runtime::local(&url, &["postgres", "postgresql"])?;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, Ordering::SeqCst);
        let schema = format!("projector_{}_{}", std::process::id(), index);
        let key = i64::from(std::process::id()) * 1000 + i64::try_from(index)?;
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&root)
            .await?;
        let options: sqlx::postgres::PgConnectOptions = url.parse()?;
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await?;
        for sql in edgeagent_inbox_postgres::PostgresInbox::MIGRATIONS
            .into_iter()
            .chain([edgeagent_audit_projector::store::MIGRATION])
        {
            sqlx::raw_sql(sql).execute(&pool).await?;
        }
        Ok(Self {
            pool,
            schema,
            key,
            root,
        })
    }
    pub fn store(&self) -> Result<edgeagent_audit_projector::store::ProjectionStore> {
        Ok(edgeagent_audit_projector::store::ProjectionStore::new(
            self.pool.clone(),
            "offline".into(),
        )?)
    }
    pub fn command(&self, mode: &str) -> std::process::Command {
        let mut command =
            std::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-audit-projector"));
        command
            .arg(mode)
            .env(
                "EDGEAGENT_POSTGRES_URL",
                std::env::var("EDGEAGENT_POSTGRES_URL").unwrap_or_default(),
            )
            .env("EDGEAGENT_PROJECTOR_SCHEMA", &self.schema)
            .env("EDGEAGENT_PROJECTOR_SCOPE", "offline");
        command
    }
    pub async fn spawn(&self, input: &[u8]) -> Result<tokio::process::Child> {
        let mut command = tokio::process::Command::from(self.command("offline-once"));
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        child.stdin.take().ok_or("stdin")?.write_all(input).await?;
        Ok(child)
    }
    pub async fn counts(&self) -> Result<(i64, i64)> {
        Ok((
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_research_views")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_inbox")
                .fetch_one(&self.pool)
                .await?,
        ))
    }
    pub async fn finish(
        self,
        result: std::result::Result<Result, Box<dyn std::any::Any + Send>>,
    ) -> Result {
        self.pool.close().await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.root)
        .await?;
        self.root.close().await;
        match result {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }
}
