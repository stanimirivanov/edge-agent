//! Credential-free internal HTTP admission and configuration bounds.
#[allow(dead_code)]
mod support;
use edgeagent_audit_projector::{
    error::{Error, Kind},
    runtime,
    store::ProjectionStore,
};
use std::sync::Arc;
use support::*;
#[tokio::test]
async fn internal_queries_reject_foreign_browser_hosts_and_writes_before_storage() -> Result {
    let pool =
        sqlx::postgres::PgPoolOptions::new().connect_lazy("postgresql://test@127.0.0.1:9/test")?;
    let server = Server::new(Arc::new(ProjectionStore::new(pool, "offline".into())?)).await?;
    for (method, path, headers, status) in [
        (
            "GET",
            "/internal/v1/research-requests/foreign/request-1",
            "",
            403,
        ),
        (
            "GET",
            "/internal/v1/research-requests/offline/request-1",
            "Origin: http://localhost\r\n",
            403,
        ),
        (
            "GET",
            "/internal/v1/research-requests/offline/request-1",
            "Host: attacker.example\r\n",
            403,
        ),
        (
            "POST",
            "/internal/v1/research-requests/offline/request-1",
            "",
            405,
        ),
    ] {
        assert!(
            server
                .query(method, path, headers)
                .await?
                .starts_with(&format!("HTTP/1.1 {status}"))
        );
    }
    Ok(())
}
#[test]
fn configuration_descriptors_and_error_diagnostics_preserve_local_boundary() -> Result {
    use std::error::Error as _;
    assert!(runtime::listen("127.0.0.1:0").is_ok());
    assert!(runtime::listen("0.0.0.0:8080").is_err());
    for url in [
        "postgresql://remote.invalid/db",
        "postgresql://127.0.0.1/db?host=remote.invalid",
    ] {
        assert!(runtime::local(url, &["postgresql"]).is_err());
    }
    let error = Error::caused(
        Kind::Storage,
        std::io::Error::other("private-test-sentinel"),
    );
    assert!(error.source().is_some());
    assert!(!format!("{error:?} {error}").contains("private-test-sentinel"));
    for mode in ["describe", "self-check", "--help", "--version"] {
        assert!(
            std::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-audit-projector"))
                .arg(mode)
                .output()?
                .status
                .success()
        );
    }
    Ok(())
}
