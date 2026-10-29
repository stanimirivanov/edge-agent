//! Gateway-owned bounded adapter to the projector's versioned internal read API.
use crate::{
    admission::{Error, Kind, valid_scope},
    http::local_request,
};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use edgeagent_contracts::research::{MAX_RESEARCH_BYTES, ResearchRecord};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};

/// HTTP-only local destination; no caller-controlled headers, routes or authority.
pub struct ProjectorClient {
    client: reqwest::Client,
    base: String,
    slots: tokio::sync::Semaphore,
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
    async fn read(
        &self,
        scope: &str,
        id: &str,
        outcome: bool,
    ) -> Result<(StatusCode, Vec<u8>), Error> {
        let _permit = self.slots.try_acquire().map_err(cause)?;
        let suffix = if outcome { "/outcome" } else { "" };
        let mut response = self
            .client
            .get(format!(
                "{}/internal/v1/research-requests/{scope}/{id}{suffix}",
                self.base
            ))
            .send()
            .await
            .map_err(cause)?;
        let status = response.status();
        if !matches!(status.as_u16(), 200 | 202 | 404 | 409)
            || response.headers().contains_key("content-encoding")
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                != Some("application/json")
        {
            return Err(unavailable());
        }
        let maximum = if outcome && status == StatusCode::OK {
            MAX_RESEARCH_BYTES
        } else {
            4096
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
        if status == StatusCode::OK && outcome {
            let record = ResearchRecord::decode(&bytes).map_err(cause)?;
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
        } else if status == StatusCode::OK {
            let view: View = serde_json::from_slice(&bytes).map_err(cause)?;
            if view.scope != scope || view.request_id != id || !view.valid() {
                return Err(unavailable());
            }
            bytes = serde_json::to_vec(&view).map_err(cause)?;
        } else {
            let state: Observation = serde_json::from_slice(&bytes).map_err(cause)?;
            let expected = match status {
                StatusCode::NOT_FOUND => "not_observed",
                StatusCode::ACCEPTED if outcome => "pending",
                StatusCode::CONFLICT if outcome => "gap",
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
}
impl View {
    fn valid(&self) -> bool {
        let key = self.applied_delivery_key.as_ref().is_some_and(|s| {
            !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())
        });
        match self.status.as_str() {
            "pending" => self.projection_revision == 0 && self.applied_delivery_key.is_none(),
            "artifact" | "abstained" => self.projection_revision == 1 && key,
            "gap" => {
                (self.projection_revision == 0 && self.applied_delivery_key.is_none())
                    || (self.projection_revision == 1 && key)
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
    outcome: bool,
) -> Response {
    if !local_request(&headers) || scope != state.scope {
        return reply(StatusCode::FORBIDDEN, br#"{"status":"forbidden"}"#.to_vec());
    }
    if !valid_scope(&scope)
        || !valid_scope(&id)
        || matches!(id.as_str(), "." | "..")
        || matches!(scope.as_str(), "." | "..")
    {
        return reply(
            StatusCode::BAD_REQUEST,
            br#"{"status":"invalid_request"}"#.to_vec(),
        );
    }
    if let Some(client) = state.client
        && let Ok(Ok((status, bytes))) =
            tokio::time::timeout(Duration::from_secs(5), client.read(&scope, &id, outcome)).await
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
    query(state, scope, id, headers, false).await
}
async fn outcome(
    State(state): State<QueryState>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    query(state, scope, id, headers, true).await
}
