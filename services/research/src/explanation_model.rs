//! Bounded local llama.cpp chat transport for the Research model port.
use edgeagent_contracts::{explanation::Proposal, explanation_profile::Deployment};
use edgeagent_research_application::explanation_model::{
    ExplanationModel, Failure, Reply, Request,
};
use edgeagent_telemetry::{
    ExplanationOutcome as Observation, ExplanationStage as Stage, record_explanation_operation,
};
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_RESPONSE_BYTES: usize = 65_536;
const MAX_PROMPT_BYTES: usize = 4096;

/// Payload-free location of the last exclusive local attempt, never authority.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStage {
    Idle,
    CheckingDeployment,
    WaitingResponse,
    ReadingResponse,
    DecodingResponse,
    ValidatingProposal,
    Complete,
}
impl ModelStage {
    fn read(value: &AtomicU8) -> Self {
        match value.load(Ordering::Relaxed) {
            1 => Self::CheckingDeployment,
            2 => Self::WaitingResponse,
            3 => Self::ReadingResponse,
            4 => Self::DecodingResponse,
            5 => Self::ValidatingProposal,
            6 => Self::Complete,
            _ => Self::Idle,
        }
    }
}

/// Trusted observation that holds an exact listener and deployment for the
/// duration of the request. The launcher must prevent endpoint replacement
/// while a lease is held.
pub trait DeploymentLease: Send {
    /// Exact observed listener.
    fn endpoint(&self) -> SocketAddrV4;
    /// Independently verified manifest and weights identities.
    fn identity(&self) -> &Deployment;
}

/// Trusted launcher boundary. A server response cannot establish identity.
pub trait DeploymentVerifier: Sync {
    /// Guard for the observed listener and deployment.
    type Lease: DeploymentLease;

    /// Verify the actual listener, runtime, configuration and weights, then
    /// keep the listener reserved until the returned guard drops.
    fn acquire(
        &self,
        endpoint: SocketAddrV4,
    ) -> impl Future<Output = Result<Self::Lease, Failure>> + Send;
}

/// One in-flight inference at a fixed IPv4 loopback endpoint.
pub struct HttpModel<V> {
    client: Client,
    endpoint: SocketAddrV4,
    verifier: V,
    prompt: String,
    prompt_id: String,
    schema: Value,
    busy: Mutex<()>,
    stage: AtomicU8,
}

impl<V: DeploymentVerifier> HttpModel<V> {
    /// Create the HTTP adapter with an operator-supplied pinned system prompt.
    ///
    /// # Errors
    /// Refuses nonlocal endpoints, empty or oversized prompts, and invalid
    /// transport configuration.
    pub fn new(endpoint: SocketAddrV4, verifier: V, prompt: String) -> Result<Self, Failure> {
        if *endpoint.ip() != Ipv4Addr::LOCALHOST
            || endpoint.port() == 0
            || prompt.trim().is_empty()
            || prompt.len() > MAX_PROMPT_BYTES
        {
            return Err(Failure::InvalidRequest);
        }
        // Reqwest can initialize TLS under workspace feature unification even
        // though this client can only dispatch to an HTTP loopback origin.
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| Failure::Unavailable)?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .tls_sslkeylogfile(false)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .http1_only()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| Failure::Unavailable)?;
        let prompt_id = Sha256::digest(prompt.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let schema = serde_json::from_slice(include_bytes!(
            "../../../crates/contracts/fixtures/explanation/proposal-v1.schema.json"
        ))
        .map_err(|_| Failure::InvalidRequest)?;
        Ok(Self {
            client,
            endpoint,
            verifier,
            prompt,
            prompt_id,
            schema,
            busy: Mutex::new(()),
            stage: AtomicU8::new(0),
        })
    }

    /// Diagnostic snapshot after sequential calls by the owned collector only.
    /// Concurrent rejected callers cannot replace the active attempt's stage.
    pub fn diagnostic_stage(&self) -> ModelStage {
        ModelStage::read(&self.stage)
    }

    async fn exchange(&self, request: &Request<'_>) -> Result<Reply, Failure> {
        let profile = request.profile();
        if self.prompt_id != profile.prompt {
            return Err(Failure::DeploymentMismatch);
        }
        let input = std::str::from_utf8(request.input()).map_err(|_| Failure::InvalidRequest)?;
        let lease = self.verifier.acquire(self.endpoint).await?;
        if lease.endpoint() != self.endpoint || lease.identity() != &profile.deployment {
            return Err(Failure::DeploymentMismatch);
        }
        let settings = profile.settings;
        let body = serde_json::to_vec(&json!({
            "model": profile.deployment.weights,
            "messages": [
                {"role": "system", "content": self.prompt},
                {"role": "user", "content": input},
            ],
            "seed": settings.seed,
            "temperature": f64::from(settings.temperature_milli) / 1000.0,
            "max_tokens": settings.max_output_tokens,
            "n": 1,
            "stream": false,
            "reasoning_effort": "none",
            "response_format": {"type": "json_schema", "schema": self.schema},
        }))
        .map_err(|_| Failure::InvalidRequest)?;
        self.stage.store(2, Ordering::Relaxed);
        let response = self
            .client
            .post(format!("http://{}/v1/chat/completions", self.endpoint))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| Failure::Unavailable)?;
        self.stage.store(3, Ordering::Relaxed);
        let proposal = read_proposal(response, &profile.deployment.weights, &self.stage).await?;
        let reply = request.accept(lease.identity().clone(), proposal);
        if reply.is_ok() {
            self.stage.store(6, Ordering::Relaxed);
        }
        drop(lease);
        reply
    }
}

impl<V: DeploymentVerifier> ExplanationModel for HttpModel<V> {
    async fn propose(&self, request: &Request<'_>) -> Result<Reply, Failure> {
        let started = Instant::now();
        let result = async {
            let _slot = self.busy.try_lock().map_err(|_| Failure::Unavailable)?;
            self.stage.store(1, Ordering::Relaxed);
            let timeout = Duration::from_millis(u64::from(request.profile().settings.timeout_ms));
            tokio::time::timeout(timeout, self.exchange(request))
                .await
                .map_err(|_| Failure::Timeout)?
        }
        .await;
        let outcome = match &result {
            Ok(_) => Observation::ProposalReceived,
            Err(Failure::Timeout) => Observation::Timeout,
            Err(Failure::BudgetExhausted) => Observation::BudgetExhausted,
            Err(Failure::Unavailable) => Observation::Unavailable,
            Err(Failure::InvalidResponse) => Observation::InvalidProposal,
            Err(_) => Observation::Failed,
        };
        record_explanation_operation(Stage::Generation, outcome, started.elapsed());
        result
    }
}

#[derive(Deserialize)]
struct Response {
    model: String,
    choices: [Choice; 1],
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: String,
    message: Message,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    role: String,
    content: Option<String>,
}

async fn read_proposal(
    mut response: reqwest::Response,
    model: &str,
    stage: &AtomicU8,
) -> Result<Vec<u8>, Failure> {
    if response.status().is_server_error() {
        return Err(Failure::Unavailable);
    }
    if response.status() != StatusCode::OK
        || response.headers().contains_key(header::CONTENT_ENCODING)
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        || response
            .content_length()
            .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Failure::InvalidResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Failure::InvalidResponse)?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(Failure::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    stage.store(4, Ordering::Relaxed);
    let response: Response = serde_json::from_slice(&body).map_err(|_| Failure::InvalidResponse)?;
    let [choice] = response.choices;
    if response.model != model || choice.message.role != "assistant" {
        return Err(Failure::InvalidResponse);
    }
    match choice.finish_reason.as_str() {
        "content_filter" => return Err(Failure::Refused),
        "stop" => {}
        _ => return Err(Failure::InvalidResponse),
    }
    let content = choice.message.content.ok_or(Failure::InvalidResponse)?;
    stage.store(5, Ordering::Relaxed);
    Proposal::from_model_json(content.as_bytes()).map_err(|_| Failure::InvalidResponse)
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn diagnostic_stages_are_payload_free_and_not_model_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let stage = std::sync::atomic::AtomicU8::new(2);
        assert_eq!(
            serde_json::to_value(ModelStage::read(&stage))?,
            "waiting_response"
        );
        stage.store(4, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            serde_json::to_value(ModelStage::read(&stage))?,
            "decoding_response"
        );
        stage.store(255, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(serde_json::to_value(ModelStage::read(&stage))?, "idle");
        Ok(())
    }
}
