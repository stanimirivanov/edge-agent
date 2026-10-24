//! Explicit creation-only local profile bootstrap; no automatic schema upgrades.
use super::*;
pub(super) async fn initialize(
    config: &config::Config,
    pool: &sqlx::PgPool,
    schema: &str,
) -> Result {
    let mut tx = pool.begin().await?;
    // Cold DDL has a separate finite allowance; the caller also caps total startup.
    sqlx::raw_sql("SET LOCAL statement_timeout = '30s'; SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    // Explicit operator-only bootstrap into a NEW private schema; never drop/rewrite it.
    // Schema identifier contains only a fixed prefix and checked hexadecimal digest.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&mut *tx)
        .await?;
    for migration in edgeagent_inbox_postgres::PostgresInbox::MIGRATIONS
        .into_iter()
        .chain(edgeagent_outbox_postgres::PostgresOutbox::MIGRATIONS)
    {
        sqlx::raw_sql(migration).execute(&mut *tx).await?;
    }
    for migration in edgeagent_market_data_postgres::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    std::fs::create_dir(config.root.join("objects"))?;
    use std::io::Write as _;
    let mut marker = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config.root.join("profile"))?;
    marker.write_all(config::MARKER)?;
    marker.sync_all()?;
    println!("offline_schema=initialized");
    Ok(())
}
