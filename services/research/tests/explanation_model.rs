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

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn performance_probe_observes_late_valid_responses_without_exporting_a_reply() -> Result {
    use edgeagent_research_application::explanation::Allowlist;
    use edgeagent_research_application::explanation_evaluation::Suite;
    let input = Suite::performance_input(1)?;
    let (listener, addr) = listener().await?;
    let mut profile = profile();
    profile.settings.timeout_ms = 1;
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    let mut body = json!({"model":profile.deployment.weights,"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":std::str::from_utf8(input.standard_proposal())?}}]});
    body["timings"] = json!({"cache_n":0,"prompt_n":4000,"predicted_n":100,"prompt_ms":600.5,"predicted_ms":200.25,"provider_text":"PRIVATE"});
    let lease = Arc::new(AtomicBool::new(false));
    let server_lease = lease.clone();
    let server = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        serve(
            listener,
            server_lease,
            http("200 OK", &serde_json::to_vec(&body)?),
        )
        .await
    });
    let adapter = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let request = Request::new(&profile, &binding, input.prepared_input().ok_or("input")?)?;
    let measurement = adapter
        .measure(&request, &input, Duration::from_secs(1))
        .await;
    let value = serde_json::to_value(measurement)?;
    assert_eq!(value["status"], "valid");
    assert_eq!(
        value["response"]["timings"],
        json!({"cache_n":0,"prompt_n":4000,"predicted_n":100,"prompt_ms":600.5,"predicted_ms":200.25})
    );
    assert_eq!(value["response"]["finish_reason"], "stop");
    assert!(!value.to_string().contains("PRIVATE"));
    assert!(value["elapsed_ms"].as_u64().is_some_and(|ms| ms >= 20));
    assert!(value.get("proposal").is_none());
    assert!(value.get("rendered").is_none());
    assert_eq!(profile.settings.timeout_ms, 1);
    assert!(!lease.load(Ordering::SeqCst));
    server.await??;
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[test]
fn performance_calibrations_are_distinct_from_the_frozen_census() -> Result {
    use edgeagent_research_application::explanation_evaluation::Suite;
    let frozen = Suite::runtime()?;
    let mut lengths = Vec::new();
    for n in 1..=3 {
        let input = Suite::performance_input(n)?;
        let bytes = input.prepared_input().ok_or("preparation")?;
        assert!(
            frozen
                .cases()
                .iter()
                .filter_map(|c| c.input())
                .all(|c| c.prepared_input() != Some(bytes))
        );
        assert_eq!(
            input.render_candidate(input.standard_proposal())?,
            input.baseline()
        );
        assert!(std::str::from_utf8(bytes)?.contains("performance-P"));
        lengths.push(bytes.len());
    }
    assert!(lengths.windows(2).all(|v| v[0] < v[1]));
    assert!(Suite::performance_input(0).is_err());
    assert!(Suite::performance_input(4).is_err());
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn performance_timeouts_release_identity_and_invalid_caps_never_dispatch() -> Result {
    use edgeagent_research_application::{explanation::Allowlist, explanation_evaluation::Suite};
    let input = Suite::performance_input(1)?;
    let (listener, addr) = listener().await?;
    let profile = profile();
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    let lease = Arc::new(AtomicBool::new(false));
    let model = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let request = Request::new(&profile, &binding, input.prepared_input().ok_or("input")?)?;
    let frozen = Suite::runtime()?;
    let grading = frozen.cases()[0].input().ok_or("grading input")?;
    let grading_request = Request::new(
        &profile,
        &binding,
        grading.prepared_input().ok_or("grading preparation")?,
    )?;
    assert_eq!(
        model
            .measure(&grading_request, grading, Duration::from_secs(1))
            .await
            .status,
        "invalid_request"
    );
    for cap in [Duration::ZERO, Duration::from_secs(181)] {
        assert_eq!(
            model.measure(&request, &input, cap).await.status,
            "invalid_request"
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(5), listener.accept())
            .await
            .is_err()
    );
    let measurement = model
        .measure(&request, &input, Duration::from_millis(20))
        .await;
    assert_eq!(measurement.status, "timeout");
    assert!(!lease.load(Ordering::SeqCst));
    let value = serde_json::json!({"kind":"research.explanation-performance.attempt.v3","measurement":measurement});
    assert!(
        serde_json::from_slice::<edgeagent_contracts::explanation_qualification::Collection>(
            &serde_json::to_vec(&value)?
        )
        .is_err()
    );
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn performance_validates_rendering_and_drops_unsupported_proposals() -> Result {
    use edgeagent_research_application::{explanation::Allowlist, explanation_evaluation::Suite};
    let input = Suite::performance_input(1)?;
    let (listener, addr) = listener().await?;
    let profile = profile();
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    let mut proposal = Proposal::decode(input.standard_proposal())?;
    proposal.claims.remove(0);
    let body = json!({"model":profile.deployment.weights,"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":std::str::from_utf8(&proposal.canonical_bytes()?)?}}]});
    let lease = Arc::new(AtomicBool::new(false));
    let server = tokio::spawn(serve(
        listener,
        lease.clone(),
        http("200 OK", &serde_json::to_vec(&body)?),
    ));
    let model = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease),
        PROMPT.into(),
    )?;
    let result = model
        .measure(
            &Request::new(&profile, &binding, input.prepared_input().ok_or("input")?)?,
            &input,
            Duration::from_secs(1),
        )
        .await;
    assert_eq!(result.status, "invalid_proposal");
    server.await??;
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn twelve_instrument_context_rejection_is_payload_free_and_preserves_the_input() -> Result {
    use edgeagent_research_application::{explanation::Allowlist, explanation_evaluation::Suite};
    let input = Suite::performance_input(3)?;
    let bytes = input.prepared_input().ok_or("preparation")?;
    let (listener, addr) = listener().await?;
    let mut profile = profile();
    profile.settings.timeout_ms = 30_000;
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    let lease = Arc::new(AtomicBool::new(false));
    let server_lease = lease.clone();
    let body = br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":24762,"n_ctx":16384,"message":"PRIVATE MUST NEVER LEAVE THE TRANSPORT"}}"#;
    let server =
        tokio::spawn(
            async move { serve(listener, server_lease, http("400 Bad Request", body)).await },
        );
    let adapter = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let request = Request::new(&profile, &binding, bytes)?;
    assert_eq!(
        adapter
            .measure(&request, &input, Duration::from_secs(601))
            .await
            .status,
        "invalid_request"
    );
    let result = serde_json::to_value(
        adapter
            .measure(&request, &input, Duration::from_secs(600))
            .await,
    )?;
    assert_eq!(result["status"], "invalid_response");
    assert_eq!(result["stage"], "reading_response");
    assert_eq!(
        result["response"],
        json!({"http_status":400,"context_capacity":{"prompt_tokens":24762,"context_tokens":16384},"timings":null,"finish_reason":null})
    );
    assert!(!result.to_string().contains("PRIVATE"));
    assert_eq!(
        server.await??["messages"][1]["content"],
        std::str::from_utf8(bytes)?
    );
    assert_eq!(profile.settings.timeout_ms, 30_000);
    assert!(!lease.load(Ordering::SeqCst));
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn context_diagnostics_refuse_malformed_or_unrelated_provider_counts() -> Result {
    use edgeagent_research_application::{explanation::Allowlist, explanation_evaluation::Suite};
    let input = Suite::performance_input(3)?;
    let profile = profile();
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    for (status, body) in [
        ("400 Bad Request", br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":true,"n_ctx":16384}}"#.to_vec()),
        ("400 Bad Request", br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":16384,"n_ctx":24762}}"#.to_vec()),
        ("400 Bad Request", br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":24762,"n_ctx":0}}"#.to_vec()),
        ("400 Bad Request", br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":4294967296,"n_ctx":16384}}"#.to_vec()),
        ("400 Bad Request", br#"{"error":{"code":400,"type":"other","n_prompt_tokens":24762,"n_ctx":16384}}"#.to_vec()),
        ("400 Bad Request", vec![b'x'; 65_537]),
        ("403 Forbidden", br#"{"error":{"code":400,"type":"exceed_context_size_error","n_prompt_tokens":24762,"n_ctx":16384}}"#.to_vec()),
    ] {
        let (listener, addr) = listener().await?;
        let lease = Arc::new(AtomicBool::new(false));
        let server_lease = lease.clone();
        // For a header-only size rejection, avoid racing a large write against
        // the client's intentional early close.
        let wire = if body.len() > 65_536 {
            b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 65537\r\n\r\n".to_vec()
        } else {
            http(status, &body)
        };
        let server = tokio::spawn(async move { serve(listener, server_lease, wire).await });
        let model = HttpModel::new(addr, Verifier(addr, profile.deployment.clone(), lease.clone()), PROMPT.into())?;
        let request = Request::new(&profile, &binding, input.prepared_input().ok_or("input")?)?;
        let result = serde_json::to_value(model.measure(&request, &input, Duration::from_secs(1)).await)?;
        assert_eq!(result["status"], "invalid_response");
        assert!(result["response"]["context_capacity"].is_null());
        assert!(!lease.load(Ordering::SeqCst));
        server.await??;
    }
    Ok(())
}

#[tokio::test]
async fn production_rejects_400_without_waiting_for_a_diagnostic_body() -> Result {
    let (listener, addr) = listener().await?;
    let profile = profile();
    let binding = binding(&profile)?;
    let lease = Arc::new(AtomicBool::new(false));
    let server_lease = lease.clone();
    let (release, hold) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        let mut length = 0;
        loop {
            line.clear();
            socket.read_line(&mut line).await?;
            if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse::<usize>()?;
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        socket.read_exact(&mut body).await?;
        assert!(server_lease.load(Ordering::SeqCst));
        socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2000\r\n\r\n").await?;
        let _ = hold.await;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let model = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        PROMPT.into(),
    )?;
    let result = model
        .propose(&Request::new(&profile, &binding, b"input")?)
        .await;
    let _ = release.send(());
    assert!(matches!(result, Err(Failure::InvalidResponse)));
    assert!(!lease.load(Ordering::SeqCst));
    server.await??;
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn malformed_optional_timings_do_not_change_proposal_acceptance() -> Result {
    use edgeagent_research_application::{explanation::Allowlist, explanation_evaluation::Suite};
    let input = Suite::performance_input(1)?;
    let profile = profile();
    let binding = Binding {
        anchor: Allowlist::from_verified(input.artifact(), input.decision())?
            .standard()?
            .anchor()
            .clone(),
        profile: profile.id()?,
        evidence: "e".repeat(64),
    };
    for timing in [
        json!({"cache_n":0,"prompt_n":true,"predicted_n":1,"prompt_ms":1,"predicted_ms":2}),
        json!({"cache_n":0,"prompt_n":1,"predicted_n":1,"prompt_ms":-1,"predicted_ms":2}),
        json!({"cache_n":0,"prompt_n":1,"predicted_n":1,"prompt_ms":5400001,"predicted_ms":2}),
        json!("PRIVATE"),
    ] {
        let (listener, addr) = listener().await?;
        let lease = Arc::new(AtomicBool::new(false));
        let server_lease = lease.clone();
        let body = json!({"model":profile.deployment.weights,"timings":timing,"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":std::str::from_utf8(input.standard_proposal())?}}]});
        let server = tokio::spawn(async move {
            serve(
                listener,
                server_lease,
                http("200 OK", &serde_json::to_vec(&body)?),
            )
            .await
        });
        let model = HttpModel::new(
            addr,
            Verifier(addr, profile.deployment.clone(), lease.clone()),
            PROMPT.into(),
        )?;
        let result = serde_json::to_value(
            model
                .measure(
                    &Request::new(&profile, &binding, input.prepared_input().ok_or("input")?)?,
                    &input,
                    Duration::from_secs(1),
                )
                .await,
        )?;
        assert_eq!(result["status"], "valid");
        assert!(result["response"]["timings"].is_null());
        assert!(!result.to_string().contains("PRIVATE"));
        server.await??;
    }
    Ok(())
}

#[cfg(feature = "performance-probe")]
#[tokio::test]
async fn development_layout_uses_bound_schema_and_the_owned_transport() -> Result {
    use edgeagent_research::explanation_model::DEVELOPMENT_LAYOUT_PROMPT;
    let (listener, addr) = listener().await?;
    let mut profile = profile();
    profile.prompt =
        edgeagent_research::explanation_owned::digest(DEVELOPMENT_LAYOUT_PROMPT.as_bytes());
    let (anchor, prepared) =
        edgeagent_research_application::explanation_evaluation::Suite::development_model_case(
            "D02",
        )?;
    let mut binding = binding(&profile)?;
    binding.anchor = anchor;
    let choice_binding = prepared["input"]
        .as_str()
        .ok_or("choice binding")?
        .to_owned();
    let input = edgeagent_contracts::canonical_json_bytes(&prepared)?;
    binding.evidence = edgeagent_research::explanation_owned::digest(&input);
    let request = Request::new(&profile, &binding, &input)?;
    let selection = json!({
        "kind": "research.explanation-development-choice.v1",
        "case": "D02", "input": choice_binding, "layout": "reader_first"
    });
    let body = serde_json::to_vec(&json!({"model":profile.deployment.weights,
        "choices":[{"finish_reason":"stop","message":{"role":"assistant","content":selection.to_string()}}]}))?;
    let lease = Arc::new(AtomicBool::new(false));
    let server_lease = lease.clone();
    let server =
        tokio::spawn(async move { serve(listener, server_lease, http("200 OK", &body)).await });
    let model = HttpModel::new(
        addr,
        Verifier(addr, profile.deployment.clone(), lease.clone()),
        DEVELOPMENT_LAYOUT_PROMPT.into(),
    )?;
    assert!(matches!(
        model
            .choose_development_layout(&request, "U01", &choice_binding)
            .await,
        Err(Failure::InvalidRequest)
    ));
    let mut foreign = prepared;
    foreign["task"] = "Altered task".into();
    let foreign = edgeagent_contracts::canonical_json_bytes(&foreign)?;
    let mut foreign_binding = binding.clone();
    foreign_binding.evidence = edgeagent_research::explanation_owned::digest(&foreign);
    let foreign_request = Request::new(&profile, &foreign_binding, &foreign)?;
    assert!(matches!(
        model
            .choose_development_layout(&foreign_request, "D02", &choice_binding)
            .await,
        Err(Failure::InvalidRequest)
    ));
    let reply = model
        .choose_development_layout(&request, "D02", &choice_binding)
        .await?;
    assert_eq!(
        serde_json::from_slice::<Value>(reply.proposal())?,
        selection
    );
    let sent = server.await??;
    assert_eq!(sent["messages"][0]["content"], DEVELOPMENT_LAYOUT_PROMPT);
    let schema = &sent["response_format"]["schema"];
    assert_eq!(schema["properties"]["case"]["const"], "D02");
    assert_eq!(schema["properties"]["input"]["const"], choice_binding);
    assert_eq!(
        schema["properties"]["layout"]["enum"],
        json!(["standard", "reader_first"])
    );
    assert_eq!(schema["additionalProperties"], false);
    assert!(!lease.load(Ordering::SeqCst));
    Ok(())
}
