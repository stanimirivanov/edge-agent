//! Gateway-owned bounded adapter to the projector's versioned internal read API.
use crate::{
    admission::{Error, Kind, valid_path_segment},
    http::local_request,
};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use edgeagent_contracts::{
    explanation::{Companion, MAX_BYTES as MAX_EXPLANATION_BYTES, Outcome},
    research::{MAX_RESEARCH_BYTES, ResearchRecord},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};

/// HTTP-only local destination; no caller-controlled headers, routes or authority.
pub struct ProjectorClient {
    client: reqwest::Client,
    base: String,
    slots: tokio::sync::Semaphore,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum ReadKind {
    Status,
    Outcome,
    Explanation,
}

fn unavailable() -> Error {
    Error::new(Kind::Storage)
}
fn cause(error: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::with_source(Kind::Storage, error)
}
impl ProjectorClient {
    /// Validate a literal loopback origin and construct a bounded non-retrying client.
    /// # Errors
    /// Rejects remote/DNS destinations, credentials, paths, overrides or client failure.
    pub fn new(endpoint: &str) -> Result<Self, Error> {
        let url = url::Url::parse(endpoint).map_err(cause)?;
        let address = url
            .host_str()
            .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .ok_or_else(unavailable)?;
        if url.scheme() != "http"
            || !address.is_loopback()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(unavailable());
        }
        let address = SocketAddr::new(
            address,
            url.port_or_known_default().ok_or_else(unavailable)?,
        );
        // Workspace feature unification can enable rustls-no-provider even for HTTP.
        // Supply the existing ring provider explicitly; this HTTP-only origin needs no roots.
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(cause)?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .tls_sslkeylogfile(false)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .http1_only()
            .http1_max_headers(32)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(4))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(cause)?;
        Ok(Self {
            client,
            base: format!("http://{address}"),
            slots: tokio::sync::Semaphore::new(8),
        })
    }
    async fn fetch(
        &self,
        scope: &str,
        id: &str,
        kind: ReadKind,
    ) -> Result<(StatusCode, Vec<u8>), Error> {
        let suffix = match kind {
            ReadKind::Status => "",
            ReadKind::Outcome => "/outcome",
            ReadKind::Explanation => "/explanation",
        };
        let request = self.client.get(format!(
            "{}/internal/v1/research-requests/{scope}/{id}{suffix}",
            self.base
        ));
        // Research's authorization and the projector's own read have bounded
        // deadlines. Leave room for them without changing the v1 read timeouts.
        let request = if kind == ReadKind::Explanation {
            request.timeout(Duration::from_secs(9))
        } else {
            request
        };
        let mut response = request.send().await.map_err(cause)?;
        let status = response.status();
        if !(matches!(status.as_u16(), 200 | 202 | 404 | 409 | 410)
            || kind == ReadKind::Explanation && status == StatusCode::FORBIDDEN)
            || (kind == ReadKind::Explanation
                && response
                    .headers()
                    .get("cache-control")
                    .is_none_or(|v| v != "no-store"))
            || response.headers().contains_key("content-encoding")
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                != Some("application/json")
        {
            return Err(unavailable());
        }
        let maximum = match (status, kind) {
            (StatusCode::OK, ReadKind::Outcome) => MAX_RESEARCH_BYTES,
            (StatusCode::OK, ReadKind::Explanation) => MAX_EXPLANATION_BYTES,
            _ => 4096,
        };
        if response
            .content_length()
            .is_some_and(|n| n > maximum as u64)
        {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(cause)? {
            if bytes.len().saturating_add(chunk.len()) > maximum {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }

    async fn read(
        &self,
        scope: &str,
        id: &str,
        kind: ReadKind,
    ) -> Result<(StatusCode, Vec<u8>), Error> {
        let _permit = self.slots.try_acquire().map_err(cause)?;
        let (status, mut bytes) = self.fetch(scope, id, kind).await?;
        if status == StatusCode::OK && kind == ReadKind::Outcome {
            decode_outcome(&bytes, scope, id)?;
        } else if status == StatusCode::OK && kind == ReadKind::Explanation {
            let companion = Companion::decode(&bytes).map_err(cause)?;
            let anchor = &companion.binding.anchor;
            if anchor.scope != scope || matches!(companion.outcome, Outcome::Refused { .. }) {
                return Err(unavailable());
            }
            // The companion contains an artifact digest rather than a request ID.
            // Corroborate it against the exact scoped outcome before returning bytes.
            let (status, artifact) = self.fetch(scope, id, ReadKind::Outcome).await?;
            if status != StatusCode::OK {
                return Err(unavailable());
            }
            let artifact = decode_outcome(&artifact, scope, id)?;
            if !matches!(artifact, ResearchRecord::Artifact(_))
                || artifact.digest().map_err(cause)? != anchor.artifact
            {
                return Err(unavailable());
            }
        } else if status == StatusCode::OK {
            let view: View = serde_json::from_slice(&bytes).map_err(cause)?;
            if view.scope != scope || view.request_id != id || !view.valid() {
                return Err(unavailable());
            }
            bytes = serde_json::to_vec(&view).map_err(cause)?;
        } else {
            let state: Observation = serde_json::from_slice(&bytes).map_err(cause)?;
            let scoped_record = kind != ReadKind::Status;
            let expected = match status {
                StatusCode::FORBIDDEN if kind == ReadKind::Explanation => "forbidden",
                StatusCode::NOT_FOUND => "not_observed",
                StatusCode::ACCEPTED if scoped_record => "pending",
                StatusCode::CONFLICT if scoped_record => "gap",
                StatusCode::GONE
                    if scoped_record
                        && matches!(
                            state.status.as_str(),
                            "invalidated" | "expired" | "withdrawn" | "corrected"
                        ) =>
                {
                    state.status.as_str()
                }
                _ => return Err(unavailable()),
            };
            if state.status != expected {
                return Err(unavailable());
            }
            bytes = serde_json::to_vec(&state).map_err(cause)?;
        }
        Ok((status, bytes))
    }
}
fn decode_outcome(bytes: &[u8], scope: &str, id: &str) -> Result<ResearchRecord, Error> {
    let record = ResearchRecord::decode(bytes).map_err(cause)?;
    let request = match &record {
        ResearchRecord::Artifact(a) => &a.request,
        ResearchRecord::Abstention(a) => &a.request,
        _ => return Err(unavailable()),
    };
    if request.scope != scope
        || request.request_id != id
        || record.canonical_bytes().map_err(cause)? != bytes
    {
        return Err(unavailable());
    }
    Ok(record)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    status: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct View {
    scope: String,
    request_id: String,
    projection_revision: u64,
    status: String,
    applied_delivery_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lifecycle: Option<edgeagent_contracts::research::ResearchLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision: Option<String>,
}
impl View {
    fn valid(&self) -> bool {
        let additional = u64::from(self.decision.is_some());
        if self.decision.as_ref().is_some_and(|v| {
            v.len() != 64
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return false;
        }
        let key = self.applied_delivery_key.as_ref().is_some_and(|s| {
            !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())
        });
        let closed = |event: &edgeagent_contracts::research::ResearchLifecycle| {
            use edgeagent_contracts::research::{LifecycleChange, ResearchRecord};
            let state = match event.change {
                LifecycleChange::Invalidated => "invalidated",
                LifecycleChange::Expired => "expired",
                LifecycleChange::Withdrawn => "withdrawn",
                LifecycleChange::Corrected { .. } => "corrected",
            };
            (2 + additional..=3 + additional).contains(&self.projection_revision)
                && key
                && event.revision == self.projection_revision - 1 - additional
                && event.scope == self.scope
                && event.request_id == self.request_id
                && (self.status == state || self.status == "gap")
                && ResearchRecord::Lifecycle(event.clone())
                    .canonical_bytes()
                    .is_ok()
        };
        match self.status.as_str() {
            "pending" => {
                self.projection_revision == 0
                    && self.decision.is_none()
                    && self.applied_delivery_key.is_none()
                    && self.lifecycle.is_none()
            }
            "artifact" | "abstained" => {
                self.projection_revision == 1 + additional && key && self.lifecycle.is_none()
            }
            "invalidated" | "expired" | "withdrawn" | "corrected" => {
                self.lifecycle.as_ref().is_some_and(closed)
            }
            "gap" => {
                if let Some(event) = &self.lifecycle {
                    closed(event)
                } else {
                    (self.projection_revision == 0
                        && self.decision.is_none()
                        && self.applied_delivery_key.is_none())
                        || (self.projection_revision == 1 + additional && key)
                }
            }
            _ => false,
        }
    }
}
#[derive(Clone)]
struct QueryState {
    scope: String,
    client: Option<Arc<ProjectorClient>>,
}
pub(crate) fn router(scope: String, client: Option<Arc<ProjectorClient>>) -> Router {
    Router::new()
        .route("/v1/research-requests/{scope}/{id}", get(status))
        .route("/v1/research-requests/{scope}/{id}/outcome", get(outcome))
        .route(
            "/v1/research-requests/{scope}/{id}/explanation",
            get(explanation),
        )
        .with_state(QueryState { scope, client })
}
fn reply(status: StatusCode, bytes: Vec<u8>) -> Response {
    (
        status,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
        ],
        bytes,
    )
        .into_response()
}
async fn query(
    state: QueryState,
    scope: String,
    id: String,
    headers: HeaderMap,
    kind: ReadKind,
) -> Response {
    if !local_request(&headers) || scope != state.scope {
        return reply(StatusCode::FORBIDDEN, br#"{"status":"forbidden"}"#.to_vec());
    }
    if !valid_path_segment(&scope) || !valid_path_segment(&id) {
        return reply(
            StatusCode::BAD_REQUEST,
            br#"{"status":"invalid_request"}"#.to_vec(),
        );
    }
    let deadline = Duration::from_secs(if kind == ReadKind::Explanation { 12 } else { 5 });
    if let Some(client) = state.client
        && let Ok(Ok((status, bytes))) =
            tokio::time::timeout(deadline, client.read(&scope, &id, kind)).await
    {
        return reply(status, bytes);
    }
    reply(
        StatusCode::SERVICE_UNAVAILABLE,
        br#"{"status":"unavailable"}"#.to_vec(),
    )
}
async fn status(
    State(state): State<QueryState>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    query(state, scope, id, headers, ReadKind::Status).await
}
async fn outcome(
    State(state): State<QueryState>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    query(state, scope, id, headers, ReadKind::Outcome).await
}

async fn explanation(
    State(state): State<QueryState>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    query(state, scope, id, headers, ReadKind::Explanation).await
}
