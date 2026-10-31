//! Real loopback HTTP proof of the Gateway/projector boundary, without databases.
use axum::{
    Router,
    extract::{Request, State},
    response::IntoResponse,
};
use edgeagent_contracts::research::ResearchRecord;
use edgeagent_gateway::{
    admission::{Accepted, AdmissionStore, Error},
    http,
    query::ProjectorClient,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Admission;
impl AdmissionStore for Admission {
    fn scope(&self) -> &str {
        "offline"
    }
    async fn admit(
        &self,
        request: edgeagent_contracts::research::ResearchRequest,
    ) -> std::result::Result<Accepted, Error> {
        Ok(Accepted::committed(&request))
    }
}
struct Server {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn new(router: Router) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self { address, task })
    }
    async fn get(&self, path: &str, headers: &str) -> Result<String> {
        tokio::time::timeout(Duration::from_secs(7), async {
            let mut socket = tokio::net::TcpStream::connect(self.address).await?;
            let host = if headers.lines().any(|line| line.starts_with("Host:")) {
                String::new()
            } else {
                format!("Host: {}\r\n", self.address)
            };
            socket
                .write_all(
                    format!("GET {path} HTTP/1.1\r\n{host}Connection: close\r\n{headers}\r\n")
                        .as_bytes(),
                )
                .await?;
            let mut result = String::new();
            socket.read_to_string(&mut result).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(result)
        })
        .await?
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Clone)]
struct Upstream {
    status: axum::http::StatusCode,
    bytes: Vec<u8>,
    content_type: &'static str,
    delay: bool,
    calls: Arc<Mutex<Vec<(String, axum::http::HeaderMap)>>>,
    entered: Arc<tokio::sync::Notify>,
}
async fn upstream(State(state): State<Upstream>, request: Request) -> axum::response::Response {
    state
        .calls
        .lock()
        .await
        .push((request.uri().to_string(), request.headers().clone()));
    state.entered.notify_one();
    if state.delay {
        std::future::pending::<()>().await;
    }
    (
        state.status,
        [
            ("content-type", state.content_type),
            ("location", "http://127.0.0.1:1/private"),
        ],
        state.bytes,
    )
        .into_response()
}
async fn setup(
    status: u16,
    bytes: Vec<u8>,
    content_type: &'static str,
    delay: bool,
) -> Result<(Server, Server, Upstream)> {
    let state = Upstream {
        status: axum::http::StatusCode::from_u16(status)?,
        bytes,
        content_type,
        delay,
        calls: Arc::default(),
        entered: Arc::default(),
    };
    let owner = Server::new(Router::new().fallback(upstream).with_state(state.clone())).await?;
    let client = Arc::new(ProjectorClient::new(&format!("http://{}", owner.address))?);
    let gateway = Server::new(http::router_with_queries(Arc::new(Admission), Some(client))).await?;
    Ok((gateway, owner, state))
}
fn body(response: &str) -> Result<&str> {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| "missing HTTP body".into())
}
const PATH: &str = "/v1/research-requests/offline/request-1";
fn view(status: &str, revision: u64, key: serde_json::Value) -> Vec<u8> {
    serde_json::json!({"scope":"offline","request_id":"request-1","projection_revision":revision,"status":status,"applied_delivery_key":key}).to_string().into_bytes()
}
#[tokio::test]
async fn query_scope_host_origin_and_invalid_ids_are_refused_before_upstream() -> Result {
    let (gateway, _owner, state) = setup(
        200,
        view("pending", 0, serde_json::Value::Null),
        "application/json",
        false,
    )
    .await?;
    for (path, headers, code) in [
        (
            "/v1/research-requests/foreign/request-1",
            "X-Forwarded-Scope: offline\r\n",
            403,
        ),
        (PATH, "Origin: http://localhost\r\n", 403),
        (PATH, "Host: attacker.example\r\n", 403),
        ("/v1/research-requests/offline/bad%20id", "", 400),
        ("/v1/research-requests/offline/..", "", 400),
    ] {
        let response = gateway.get(path, headers).await?;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {code}")),
            "{response}"
        );
        assert!(response.contains("cache-control: no-store"));
    }
    assert!(state.calls.lock().await.is_empty());
    let missing = Server::new(http::router(Arc::new(Admission))).await?;
    assert!(missing.get(PATH, "").await?.starts_with("HTTP/1.1 503"));
    Ok(())
}
#[tokio::test]
async fn queries_preserve_observations_and_exact_terminal_bytes_without_forwarded_identity()
-> Result {
    for (status, revision, key) in [
        ("pending", 0, serde_json::Value::Null),
        ("artifact", 1, "opaque-1".into()),
        ("abstained", 1, "opaque-2".into()),
        ("gap", 1, "opaque-3".into()),
        ("gap", 0, serde_json::Value::Null),
    ] {
        let bytes = view(status, revision, key);
        let (gateway, _owner, state) = setup(200, bytes.clone(), "application/json", false).await?;
        let response = gateway
            .get(
                PATH,
                "Authorization: private-test-sentinel\r\nX-Forwarded-Host: attacker.example\r\n",
            )
            .await?;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body(&response)?)?,
            serde_json::from_slice::<serde_json::Value>(&bytes)?
        );
        let calls = state.calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            "/internal/v1/research-requests/offline/request-1"
        );
        assert!(!calls[0].1.contains_key("authorization"));
        assert!(!calls[0].1.contains_key("x-forwarded-host"));
    }
    for status in ["artifact", "abstained", "gap"] {
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&view(status, 2, "paired".into()))?;
        metadata["decision"] = "ab".repeat(32).into();
        let (gateway, _owner, _) = setup(
            200,
            serde_json::to_vec(&metadata)?,
            "application/json",
            false,
        )
        .await?;
        let response = gateway.get(PATH, "").await?;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(body(&response)?.contains(&"ab".repeat(32)));
    }
    for (status, change) in [
        (
            "invalidated",
            serde_json::json!({"transition":"invalidated"}),
        ),
        ("expired", serde_json::json!({"transition":"expired"})),
        ("withdrawn", serde_json::json!({"transition":"withdrawn"})),
        (
            "corrected",
            serde_json::json!({"transition":"corrected","replacement":"03".repeat(32)}),
        ),
    ] {
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&view(status, 2, "latest".into()))?;
        metadata["lifecycle"] = serde_json::json!({"scope":"offline","request_id":"request-1","artifact":"02".repeat(32),"revision":1,"effective_at_ms":200,"change":change});
        let (gateway, _owner, _) = setup(
            200,
            serde_json::to_vec(&metadata)?,
            "application/json",
            false,
        )
        .await?;
        let response = gateway.get(PATH, "").await?;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(body(&response)?.contains(status));
        let (gateway, _owner, _) = setup(
            410,
            format!("{{\"status\":\"{status}\"}}").into_bytes(),
            "application/json",
            false,
        )
        .await?;
        assert!(
            gateway
                .get(&format!("{PATH}/outcome"), "")
                .await?
                .starts_with("HTTP/1.1 410")
        );
    }
    for (code, status, suffix) in [
        (202, "pending", "/outcome"),
        (409, "gap", "/outcome"),
        (404, "not_observed", ""),
        (404, "not_observed", "/outcome"),
    ] {
        let bytes = format!("{{\"status\":\"{status}\"}}").into_bytes();
        let (gateway, _owner, _) = setup(code, bytes.clone(), "application/json", false).await?;
        let response = gateway.get(&format!("{PATH}{suffix}"), "").await?;
        assert!(response.starts_with(&format!("HTTP/1.1 {code}")));
        assert_eq!(body(&response)?.as_bytes(), bytes);
    }
    for bytes in [
        include_bytes!("../../../crates/contracts/fixtures/research/v1/artifact.json").as_slice(),
        include_bytes!("../../../crates/contracts/fixtures/research/v1/abstention.json").as_slice(),
    ] {
        let mut record = ResearchRecord::decode(bytes)?;
        match &mut record {
            ResearchRecord::Artifact(a) => a.request.request_id = "request-1".into(),
            ResearchRecord::Abstention(a) => a.request.request_id = "request-1".into(),
            _ => return Err("terminal".into()),
        }
        let bytes = record.canonical_bytes()?;
        let (gateway, _owner, state) = setup(200, bytes.clone(), "application/json", false).await?;
        let response = gateway.get(&format!("{PATH}/outcome"), "").await?;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(body(&response)?.as_bytes(), bytes);
        assert!(response.contains("cache-control: no-store"));
        assert!(state.calls.lock().await[0].0.ends_with("/outcome"));
    }
    Ok(())
}
#[tokio::test]
async fn invalid_oversized_redirected_and_unavailable_projector_responses_fail_closed() -> Result {
    let mut foreign: serde_json::Value =
        serde_json::from_slice(&view("artifact", 1, "key".into()))?;
    foreign["scope"] = "foreign".into();
    let mut wrong_id = foreign.clone();
    wrong_id["scope"] = "offline".into();
    wrong_id["request_id"] = "other".into();
    for (code, bytes, kind, suffix) in [
        (
            200,
            b"private-test-sentinel".to_vec(),
            "application/json",
            "",
        ),
        (200, serde_json::to_vec(&foreign)?, "application/json", ""),
        (200, serde_json::to_vec(&wrong_id)?, "application/json", ""),
        (
            200,
            view("artifact", 0, "key".into()),
            "application/json",
            "",
        ),
        (
            200,
            view("pending", 0, "key".into()),
            "application/json",
            "",
        ),
        (
            200,
            view("artifact", 1, serde_json::Value::Null),
            "application/json",
            "",
        ),
        (
            200,
            view("withdrawn", 1, "key".into()),
            "application/json",
            "",
        ),
        (200, vec![b' '; 4097], "application/json", ""),
        (200, vec![b' '; 131073], "application/json", "/outcome"),
        (200, b"{}".to_vec(), "text/html", ""),
        (
            200,
            include_bytes!("../../../crates/contracts/fixtures/research/v1/artifact.json").to_vec(),
            "application/json",
            "/outcome",
        ),
        (
            202,
            br#"{"status":"pending"}"#.to_vec(),
            "application/json",
            "",
        ),
        (
            404,
            br#"{"status":"rejected"}"#.to_vec(),
            "application/json",
            "",
        ),
        (
            409,
            br#"{"status":"gap","secret":"private-test-sentinel"}"#.to_vec(),
            "application/json",
            "/outcome",
        ),
        (302, Vec::new(), "application/json", ""),
        (
            503,
            b"private-test-sentinel".to_vec(),
            "application/json",
            "",
        ),
    ] {
        let (gateway, _owner, state) = setup(code, bytes, kind, false).await?;
        let response = gateway.get(&format!("{PATH}{suffix}"), "").await?;
        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert_eq!(body(&response)?, r#"{"status":"unavailable"}"#);
        assert_eq!(state.calls.lock().await.len(), 1);
    }
    // No Content-Length: the streamed-body limit must independently reject excess bytes.
    let chunked = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", chunked.local_addr()?);
    let sender = tokio::spawn(async move {
        let (mut socket, _) = chunked.accept().await?;
        let mut request = [0; 2048];
        if socket.read(&mut request).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        let bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1001\r\n{}\r\n0\r\n\r\n",
            " ".repeat(4097)
        );
        socket.write_all(bytes.as_bytes()).await?;
        Ok::<_, std::io::Error>(())
    });
    let gateway = Server::new(http::router_with_queries(
        Arc::new(Admission),
        Some(Arc::new(ProjectorClient::new(&endpoint)?)),
    ))
    .await?;
    assert!(gateway.get(PATH, "").await?.starts_with("HTTP/1.1 503"));
    sender.await??;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    drop(listener);
    let gateway = Server::new(http::router_with_queries(
        Arc::new(Admission),
        Some(Arc::new(ProjectorClient::new(&format!(
            "http://{address}"
        ))?)),
    ))
    .await?;
    assert!(gateway.get(PATH, "").await?.starts_with("HTTP/1.1 503"));
    Ok(())
}
#[tokio::test]
async fn stalled_projector_is_bounded_and_configuration_cannot_redirect_authority() -> Result {
    for url in [
        "https://127.0.0.1:8081",
        "http://example.org",
        "http://localhost:8081",
        "http://192.0.2.1",
        "http://user:private-test-sentinel@127.0.0.1",
        "http://127.0.0.1/private",
        "http://127.0.0.1?target=remote",
        "http://127.0.0.1#fragment",
    ] {
        let error = ProjectorClient::new(url)
            .err()
            .ok_or("accepted invalid config")?;
        assert!(!format!("{error:?} {error}").contains("private-test-sentinel"));
    }
    assert!(ProjectorClient::new("http://[::1]:8081").is_ok());
    let (gateway, _owner, state) = setup(200, Vec::new(), "application/json", true).await?;
    let gateway = Arc::new(gateway);
    let mut waiting = Vec::new();
    for _ in 0..8 {
        let gateway = gateway.clone();
        waiting.push(tokio::spawn(async move { gateway.get(PATH, "").await }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.calls.lock().await.len() < 8 {
            state.entered.notified().await;
        }
    })
    .await?;
    // Saturation returns immediately, without a ninth request or an unbounded queue.
    let response = tokio::time::timeout(Duration::from_secs(1), gateway.get(PATH, "")).await??;
    assert!(response.starts_with("HTTP/1.1 503"));
    assert_eq!(state.calls.lock().await.len(), 8);
    for request in waiting {
        assert!(request.await??.starts_with("HTTP/1.1 503"));
    }
    Ok(())
}
