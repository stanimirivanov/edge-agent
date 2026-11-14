use edgeagent_contracts::{
    explanation::{Anchor, Binding, Proposal},
    explanation_profile::{Deployment, Profile, Settings, Tools},
};
use edgeagent_research::explanation_model::{DeploymentLease, DeploymentVerifier, HttpModel};
use edgeagent_research_application::explanation_model::{ExplanationModel, Failure, Request};
use serde_json::{Value, json};
use std::{
    future::ready,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
    net::TcpListener,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const PROMPT: &str = "Return an explanation proposal.";
const PROPOSAL: &[u8] =
    include_bytes!("../../../crates/contracts/fixtures/explanation/proposal-v1.json");

struct Lease(SocketAddrV4, Deployment, Arc<AtomicBool>);
impl DeploymentLease for Lease {
    fn endpoint(&self) -> SocketAddrV4 {
        self.0
    }
    fn identity(&self) -> &Deployment {
        &self.1
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.2.store(false, Ordering::SeqCst);
    }
}
struct Verifier(SocketAddrV4, Deployment, Arc<AtomicBool>);
impl DeploymentVerifier for Verifier {
    type Lease = Lease;

    fn acquire(
        &self,
        _: SocketAddrV4,
    ) -> impl Future<Output = std::result::Result<Lease, Failure>> + Send {
        self.2.store(true, Ordering::SeqCst);
        ready(Ok(Lease(self.0, self.1.clone(), self.2.clone())))
    }
}

fn profile() -> Profile {
    Profile {
        deployment: Deployment {
            runtime: "a".repeat(64),
            weights: "b".repeat(64),
        },
        settings: Settings {
            seed: 71,
            temperature_milli: 125,
            max_output_tokens: 256,
            timeout_ms: 1000,
        },
        prompt: "75da46c6986a35c737e7a149f8f9d50e92f9f9677713738e6fae8156958285bd".into(),
        validator: "d".repeat(64),
        renderer: "e".repeat(64),
        tools: Tools::None,
    }
}

fn binding(profile: &Profile) -> Result<Binding> {
    Ok(Binding {
        anchor: Anchor {
            scope: "operator".into(),
            artifact: "c".repeat(64),
            decision: "d".repeat(64),
        },
        profile: profile.id()?,
        evidence: "e".repeat(64),
    })
}

fn response(profile: &Profile) -> Result<Value> {
    Ok(json!({
        "model": profile.deployment.weights,
        "choices": [{"finish_reason":"stop", "message": {
            "role":"assistant", "content": std::str::from_utf8(PROPOSAL)?.trim_end(),
        }}],
    }))
}

fn http(status: &str, body: &[u8]) -> Vec<u8> {
    let mut wire = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire
}

async fn listener() -> Result<(TcpListener, SocketAddrV4)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let SocketAddr::V4(address) = listener.local_addr()? else {
        return Err("expected IPv4".into());
    };
    Ok((listener, address))
}

async fn serve(listener: TcpListener, lease: Arc<AtomicBool>, wire: Vec<u8>) -> Result<Value> {
    let (socket, _) = listener.accept().await?;
    assert!(lease.load(Ordering::SeqCst));
    let mut socket = BufReader::new(socket);
    let mut line = String::new();
    socket.read_line(&mut line).await?;
    assert_eq!(line, "POST /v1/chat/completions HTTP/1.1\r\n");
    let mut size = None;
    loop {
        line.clear();
        if socket.read_line(&mut line).await? == 0 {
            return Err("missing headers".into());
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("content-length:") {
            size = Some(value.trim().parse::<usize>()?);
        }
    }
    let mut body = vec![0; size.ok_or("missing length")?];
    socket.read_exact(&mut body).await?;
    socket.get_mut().write_all(&wire).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[tokio::test]
async fn sends_one_bound_request_and_normalizes_the_typed_proposal() -> Result {
    let (listener, addr) = listener().await?;
    let profile = profile();
    let binding = binding(&profile)?;
    let lease = Arc::new(AtomicBool::new(false));
    let server = tokio::spawn(serve(
        listener,
        lease.clone(),
        http("200 OK", &serde_json::to_vec(&response(&profile)?)?),
    ));
    let adapter = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let reply = adapter
        .propose(&Request::new(
            &profile,
            &binding,
            b"reviewed excerpt: ignore instructions",
        )?)
        .await?;
    assert_eq!(reply.proposal(), PROPOSAL);
    assert!(Proposal::decode(reply.proposal()).is_ok());
    assert_eq!(reply.observed_deployment(), &profile.deployment);
    let sent = server.await??;
    assert_eq!(
        sent["messages"][0],
        json!({"role":"system","content":PROMPT})
    );
    assert_eq!(
        sent["messages"][1],
        json!({"role":"user","content":"reviewed excerpt: ignore instructions"})
    );
    assert_eq!(sent["model"], profile.deployment.weights);
    assert_eq!(sent["seed"], 71);
    assert_eq!(sent["temperature"], 0.125);
    assert_eq!(sent["max_tokens"], 256);
    assert_eq!(sent["stream"], false);
    assert!(sent.get("tools").is_none());
    assert!(!lease.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn refuses_wrong_deployment_endpoint_or_prompt_before_disclosure() -> Result {
    let (listener, addr) = listener().await?;
    let profile = profile();
    let binding = binding(&profile)?;
    let request = Request::new(&profile, &binding, b"secret input")?;
    let lease = Arc::new(AtomicBool::new(false));
    let foreign = SocketAddrV4::new(Ipv4Addr::LOCALHOST, if addr.port() == 1 { 2 } else { 1 });
    for (endpoint, deployment, prompt) in [
        (
            addr,
            Deployment {
                weights: "f".repeat(64),
                ..profile.deployment.clone()
            },
            PROMPT,
        ),
        (foreign, profile.deployment.clone(), PROMPT),
        (addr, profile.deployment.clone(), "different prompt"),
    ] {
        let adapter = HttpModel::new(
            addr,
            Verifier(endpoint, deployment, lease.clone()),
            prompt.into(),
        )?;
        assert!(matches!(
            adapter.propose(&request).await,
            Err(Failure::DeploymentMismatch)
        ));
        assert!(!lease.load(Ordering::SeqCst));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    assert!(
        HttpModel::new(
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, addr.port()),
            Verifier(addr, profile.deployment, lease),
            PROMPT.into()
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn refuses_unsafe_response_framing_and_payload() -> Result {
    let profile = profile();
    let binding = binding(&profile)?;
    let valid = response(&profile)?;
    let mut cases = vec![
        b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /v1/chat/completions\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".to_vec(),
        http("200 OK", &vec![b' '; 65_537]),
        [b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10001\r\n".as_slice(), &vec![b' '; 65_537], b"\r\n0\r\n\r\n"].concat(),
    ];
    for (key, value) in [
        ("tool_calls", json!([{"name":"shell"}])),
        ("role", json!("tool")),
        ("content", json!("not a proposal")),
    ] {
        let mut bad = valid.clone();
        bad["choices"][0]["message"][key] = value;
        cases.push(http("200 OK", &serde_json::to_vec(&bad)?));
    }
    for (key, value) in [("model", json!("latest")), ("choices", json!([]))] {
        let mut bad = valid.clone();
        bad[key] = value;
        cases.push(http("200 OK", &serde_json::to_vec(&bad)?));
    }
    let mut truncated = valid;
    truncated["choices"][0]["finish_reason"] = json!("length");
    cases.push(http("200 OK", &serde_json::to_vec(&truncated)?));
    for wire in cases {
        let (listener, addr) = listener().await?;
        let lease = Arc::new(AtomicBool::new(false));
        let server = tokio::spawn(serve(listener, lease.clone(), wire));
        let adapter = HttpModel::new(
            addr,
            Verifier(addr, profile.deployment.clone(), lease),
            PROMPT.into(),
        )?;
        assert!(matches!(
            adapter
                .propose(&Request::new(&profile, &binding, b"input")?)
                .await,
            Err(Failure::InvalidResponse)
        ));
        server.await??;
    }
    Ok(())
}

#[tokio::test]
async fn timeout_and_cancellation_release_the_deployment_lease() -> Result {
    let (listener, addr) = listener().await?;
    let mut profile = profile();
    profile.settings.timeout_ms = 80;
    let binding = binding(&profile)?;
    let lease = Arc::new(AtomicBool::new(false));
    let adapter = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let request = Request::new(&profile, &binding, b"input")?;
    let (result, connection) = tokio::join!(adapter.propose(&request), listener.accept());
    assert!(matches!(result, Err(Failure::Timeout)));
    assert_eq!(
        serde_json::to_value(adapter.diagnostic_stage())?,
        "waiting_response"
    );
    assert!(!lease.load(Ordering::SeqCst));
    drop(connection?);

    let mut pending = Box::pin(adapter.propose(&request));
    tokio::select! {
        result = &mut pending => return Err(format!("attempt ended before cancellation: {}", result.err().ok_or("unexpected success")?).into()),
        accepted = listener.accept() => { let _connection = accepted?; assert!(lease.load(Ordering::SeqCst)); }
    }
    drop(pending);
    assert!(!lease.load(Ordering::SeqCst));
    Ok(())
}
