#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema administration."
)]
use super::Result;
use edgeagent_gateway::admission::MIGRATION;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};
pub struct Harness {
    pub pool: PgPool,
    root: PgPool,
    options: PgConnectOptions,
    schema: String,
}
impl Harness {
    pub async fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let options = PgConnectOptions::from_str(&std::env::var("EDGEAGENT_POSTGRES_URL")?)?;
        if options.get_host() != "localhost"
            && !options
                .get_host()
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        {
            return Err("test database must be loopback".into());
        }
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        let schema = format!(
            "gateway_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&root)
            .await?;
        let options = options.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options.clone())
            .await?;
        for sql in edgeagent_outbox_postgres::PostgresOutbox::MIGRATIONS
            .into_iter()
            .chain([MIGRATION])
        {
            sqlx::raw_sql(sql).execute(&pool).await?;
        }
        Ok(Self {
            pool,
            root,
            options,
            schema,
        })
    }
    pub async fn reconnect(&self) -> Result<PgPool> {
        Ok(PgPoolOptions::new()
            .max_connections(2)
            .connect_with(self.options.clone())
            .await?)
    }
    pub async fn counts(&self) -> Result<(i64, i64)> {
        Ok((
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_gateway_requests")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox")
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
