//! Real loopback boundary for current Research authorization decisions.
use edgeagent_research::explanation_authority::{Decision, router};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

type Result = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn scope_binding_and_each_read_require_current_authority() -> Result {
    let revoked = Arc::new(AtomicBool::new(false));
    let state = revoked.clone();
    let app = router("offline".into(), "local-research".into(), move |read| {
        let state = state.clone();
        async move {
            if read.request_id == "stalled" {
                std::future::pending::<Decision>().await
            } else if read.request_id == "unavailable" {
                Decision::Unavailable
            } else if state.load(Ordering::SeqCst) || read.companion != "a".repeat(64) {
                Decision::Denied
            } else {
                Decision::Allowed
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .no_proxy()
        .timeout(std::time::Duration::from_secs(7))
        .build()?;
    let read = |scope: &str, id: &str, audience: &str, digest: &str| {
        client
            .post(format!("{origin}/internal/v1/explanation-authorizations"))
            .header("content-type", "application/json")
            .body(
                serde_json::json!({
                    "scope": scope,
                    "request_id": id,
                    "audience": audience,
                    "companion": digest,
                })
                .to_string(),
            )
    };

    let response = read("offline", "request", "local-research", &"a".repeat(64))
        .send()
        .await?;
    assert_eq!(response.status(), 204);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(response.bytes().await?.is_empty());
    for (scope, id, audience, digest, expected) in [
        ("foreign", "request", "local-research", "a", 403),
        ("offline", "request", "foreign", "a", 403),
        ("offline", "request", "local-research", "b", 403),
        ("offline", "..", "local-research", "a", 400),
        ("offline", "unavailable", "local-research", "a", 503),
        ("offline", "stalled", "local-research", "a", 503),
    ] {
        let response = read(scope, id, audience, &digest.repeat(64)).send().await?;
        assert_eq!(response.status(), expected);
        assert!(response.bytes().await?.is_empty());
    }
    let browser = read("offline", "request", "local-research", &"a".repeat(64))
        .header("origin", "http://127.0.0.1")
        .send()
        .await?;
    assert_eq!(browser.status(), 403);
    revoked.store(true, Ordering::SeqCst);
    assert_eq!(
        read("offline", "request", "local-research", &"a".repeat(64))
            .send()
            .await?
            .status(),
        403
    );
    server.abort();
    Ok(())
}
