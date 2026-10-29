//! Credential-free HTTP and configuration refusal proofs.
mod support;
use edgeagent_contracts::research::ResearchRequest;
use edgeagent_gateway::{
    admission::{Accepted, AdmissionStore, Error, Kind},
    runtime::Config,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;
const JSON: &str = "Content-Type: application/json\r\n";
struct Store {
    calls: AtomicUsize,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    wait: bool,
    error: Option<Kind>,
}
impl Store {
    fn new(wait: bool, error: Option<Kind>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            wait,
            error,
        })
    }
}
impl AdmissionStore for Store {
    fn scope(&self) -> &str {
        "operator"
    }
    async fn admit(&self, r: ResearchRequest) -> std::result::Result<Accepted, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if self.wait {
            self.release.notified().await;
        }
        if let Some(kind) = self.error {
            return Err(Error::with_source(
                kind,
                std::io::Error::other("private-test-sentinel"),
            ));
        }
        Ok(Accepted::committed(&r))
    }
}
#[tokio::test]
async fn acknowledgement_waits_for_durable_admission() -> Result {
    let store = Store::new(true, None);
    let server = Server::new(store.clone()).await?;
    let body = serde_json::to_vec(&request())?;
    let address = server.address;
    let pending = tokio::spawn(async move { post(address, &body, JSON).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), store.entered.notified()).await?;
    assert!(!pending.is_finished());
    store.release.notify_one();
    let response = pending.await??;
    assert!(response.starts_with("HTTP/1.1 202"));
    assert!(
        response.contains(r#"{"request_id":"request-1","scope":"operator","status":"accepted"}"#)
    );
    Ok(())
}
#[tokio::test]
async fn malformed_foreign_browser_and_oversized_inputs_never_reach_storage() -> Result {
    let store = Store::new(false, None);
    let server = Server::new(store.clone()).await?;
    let good = serde_json::to_vec(&request())?;
    let mut foreign = request();
    foreign.scope = "foreign".into();
    let mut invalid = request();
    invalid.instruments = vec!["zero".into()];
    let mut unknown = serde_json::to_value(request())?;
    unknown["extra"] = true.into();
    let cases = vec![
        (
            good.clone(),
            "Content-Type: application/json\r\nHost: attacker.example\r\n",
            403,
        ),
        (b"{}".to_vec(), JSON, 400),
        (serde_json::to_vec(&foreign)?, JSON, 403),
        (serde_json::to_vec(&invalid)?, JSON, 400),
        (serde_json::to_vec(&unknown)?, JSON, 400),
        (good.clone(), "Content-Type: text/plain\r\n", 415),
        (
            good.clone(),
            "Content-Type: application/json\r\nOrigin: http://localhost\r\n",
            403,
        ),
        (
            vec![b' '; edgeagent_contracts::research::MAX_RESEARCH_BYTES + 1],
            JSON,
            413,
        ),
    ];
    for (body, headers, status) in cases {
        assert!(
            server
                .post(&body, headers)
                .await?
                .starts_with(&format!("HTTP/1.1 {status}"))
        );
    }
    assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    Ok(())
}
#[tokio::test]
async fn conflicts_and_unavailable_commits_return_safe_non_success_responses() -> Result {
    for (kind, status) in [(Kind::Conflict, 409), (Kind::Storage, 503)] {
        let server = Server::new(Store::new(false, Some(kind))).await?;
        let response = server.post(&serde_json::to_vec(&request())?, JSON).await?;
        assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(!response.contains("private-test-sentinel"));
    }
    Ok(())
}
#[test]
fn serving_requires_explicit_loopback_database_schema_and_scope() {
    fn config(name: &str) -> Option<String> {
        Some(
            match name {
                "EDGEAGENT_GATEWAY_LISTEN" => "127.0.0.1:8080",
                "EDGEAGENT_POSTGRES_URL" => "postgresql://test@127.0.0.1:5432/test",
                "EDGEAGENT_GATEWAY_SCHEMA" => "gateway_local",
                "EDGEAGENT_GATEWAY_SCOPE" => "operator",
                _ => return None,
            }
            .into(),
        )
    }
    assert!(Config::parse(config).is_ok());
    assert!(
        Config::parse(|key| if key == "EDGEAGENT_GATEWAY_PROJECTOR_URL" {
            Some("http://127.0.0.1:8081".into())
        } else {
            config(key)
        })
        .is_ok()
    );
    for (key, value) in [
        ("EDGEAGENT_GATEWAY_LISTEN", "0.0.0.0:8080"),
        (
            "EDGEAGENT_POSTGRES_URL",
            "postgresql://test@example.org/test",
        ),
        (
            "EDGEAGENT_POSTGRES_URL",
            "postgresql://test@127.0.0.1/test?host=example.org",
        ),
        ("EDGEAGENT_GATEWAY_PROJECTOR_URL", "http://example.org"),
        ("EDGEAGENT_GATEWAY_SCHEMA", "public"),
        ("EDGEAGENT_GATEWAY_SCOPE", ""),
    ] {
        assert!(
            Config::parse(|k| if k == key {
                Some(value.into())
            } else {
                config(k)
            })
            .is_err()
        );
    }
    for key in [
        "EDGEAGENT_GATEWAY_LISTEN",
        "EDGEAGENT_POSTGRES_URL",
        "EDGEAGENT_GATEWAY_SCHEMA",
        "EDGEAGENT_GATEWAY_SCOPE",
    ] {
        assert!(Config::parse(|k| if k == key { None } else { config(k) }).is_err());
    }
}
#[test]
fn descriptor_commands_remain_available_without_serving_configuration() -> Result {
    for argument in ["describe", "self-check", "--help", "--version"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_edgeagent-gateway"))
            .arg(argument)
            .output()?;
        assert!(output.status.success(), "{argument}");
    }
    Ok(())
}
