#![allow(
    clippy::disallowed_methods,
    reason = "Isolated owner schema administration."
)]
use super::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::atomic::{AtomicUsize, Ordering};
pub struct Database {
    pub pool: PgPool,
    pub schema: String,
    root: PgPool,
}
impl Database {
    pub async fn new() -> Result<Self> {
        let url = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        edgeagent_research::runtime::local(&url, &["postgres", "postgresql"])?;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let schema = format!(
            "research_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&root)
            .await?;
        let options: sqlx::postgres::PgConnectOptions = url.parse()?;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.options([("search_path", schema.as_str())]))
            .await?;
        for sql in edgeagent_inbox_postgres::PostgresInbox::MIGRATIONS
            .into_iter()
            .chain(edgeagent_outbox_postgres::PostgresOutbox::MIGRATIONS)
            .chain([
                edgeagent_research_postgres::MIGRATION,
                edgeagent_research_postgres::lifecycle::MIGRATION,
                edgeagent_research_postgres::governance::MIGRATION,
                edgeagent_research_postgres::DECISION_MIGRATION,
            ])
        {
            sqlx::raw_sql(sql).execute(&pool).await?;
        }
        Ok(Self { pool, schema, root })
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
