//! Explicit loopback serving configuration; schema preparation is an operator action.
use crate::{
    admission::{PostgresAdmission, valid_scope},
    http,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
/// Validated local profile; Debug is deliberately unavailable for credential safety.
pub struct Config {
    listen: SocketAddr,
    database: PgConnectOptions,
    scope: String,
}
impl Config {
    /// Parse configuration without reading process-global environment in tests.
    /// # Errors
    /// Returns only static, credential-free configuration diagnostics.
    pub fn parse(read: impl Fn(&str) -> Option<String>) -> Result<Self, &'static str> {
        let listen = read("EDGEAGENT_GATEWAY_LISTEN")
            .ok_or("missing listen address")?
            .parse::<SocketAddr>()
            .map_err(|_| "invalid listen address")?;
        if !listen.ip().is_loopback() {
            return Err("gateway requires loopback");
        }
        let database = read("EDGEAGENT_POSTGRES_URL").ok_or("missing database URL")?;
        let url = url::Url::parse(&database).map_err(|_| "invalid database URL")?;
        if !["postgres", "postgresql"].contains(&url.scheme())
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.host_str().is_some_and(|s| {
                s == "localhost"
                    || s.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
        {
            return Err("database requires a loopback URL without overrides");
        }
        let schema = read("EDGEAGENT_GATEWAY_SCHEMA").ok_or("missing gateway schema")?;
        if !schema.starts_with("gateway_")
            || schema.len() > 63
            || !schema
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("invalid gateway schema");
        }
        let scope = read("EDGEAGENT_GATEWAY_SCOPE").ok_or("missing caller scope")?;
        if !valid_scope(&scope) {
            return Err("invalid caller scope");
        }
        let database = PgConnectOptions::from_str(&database)
            .map_err(|_| "invalid database URL")?
            .options([("search_path", schema.as_str())]);
        Ok(Self {
            listen,
            database,
            scope,
        })
    }
}
/// Serve only an explicitly configured local operator; never runs migrations.
/// # Errors
/// Startup/storage/listener errors are deliberately redacted.
pub async fn serve(config: Config) -> Result<(), &'static str> {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(config.database)
        .await
        .map_err(|_| "gateway database unavailable")?;
    let ready=sqlx::query_scalar!(r#"SELECT to_regclass('edgeagent_gateway_requests') IS NOT NULL AND to_regclass('edgeagent_message_outbox') IS NOT NULL AS "ready!""#).fetch_one(&pool).await.map_err(|_| "gateway schema unavailable")?;
    if !ready {
        return Err("gateway schema is not prepared");
    }
    let store =
        PostgresAdmission::new(pool.clone(), config.scope).map_err(|_| "invalid gateway scope")?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|_| "gateway listener unavailable")?;
    let result = axum::serve(listener, http::router(Arc::new(store)))
        .with_graceful_shutdown(shutdown())
        .await;
    pool.close().await;
    result.map_err(|_| "gateway server failed")
}
async fn shutdown() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return;
    };
    tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=terminate.recv()=>{} }
}
