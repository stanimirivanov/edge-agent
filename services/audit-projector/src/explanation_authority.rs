//! Bounded client for the Research owner's one-read decision.
use crate::error::{Error, Kind};
use edgeagent_contracts::evidence::EvidenceId;
use reqwest::{StatusCode, header};
use std::{net::SocketAddr, sync::Arc, time::Duration};

pub(crate) struct Client {
    http: reqwest::Client,
    endpoint: String,
    audience: EvidenceId,
    slots: tokio::sync::Semaphore,
}

fn unavailable() -> Error {
    Error::new(Kind::Storage)
}

impl Client {
    pub(crate) fn new(origin: &str, audience: &str) -> Result<Self, Error> {
        let url = url::Url::parse(origin).map_err(|_| unavailable())?;
        let ip = url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .filter(std::net::IpAddr::is_loopback)
            .ok_or_else(unavailable)?;
        if url.scheme() != "http"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(unavailable());
        }
        let address = SocketAddr::new(ip, url.port_or_known_default().ok_or_else(unavailable)?);
        let audience = EvidenceId::new(audience).map_err(|_| unavailable())?;
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| unavailable())?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .tls_sslkeylogfile(false)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .http1_max_headers(32)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(6))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            http,
            endpoint: format!("http://{address}/internal/v1/explanation-authorizations"),
            audience,
            slots: tokio::sync::Semaphore::new(4),
        })
    }

    pub(crate) async fn allows(&self, scope: &str, id: &str, digest: &str) -> Result<bool, Error> {
        let _slot = self.slots.try_acquire().map_err(|_| unavailable())?;
        let body = serde_json::json!({
            "scope": scope,
            "request_id": id,
            "audience": self.audience.as_str(),
            "companion": digest,
        });
        let mut response = self
            .http
            .post(&self.endpoint)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| unavailable())?;
        let allowed = match response.status() {
            StatusCode::NO_CONTENT => true,
            StatusCode::FORBIDDEN => false,
            _ => return Err(unavailable()),
        };
        let headers = response.headers();
        if headers
            .get(header::CACHE_CONTROL)
            .is_none_or(|v| v != "no-store")
            || headers.contains_key(header::CONTENT_ENCODING)
            || headers.contains_key(header::TRANSFER_ENCODING)
            || headers
                .get(header::CONTENT_LENGTH)
                .is_some_and(|v| v != "0")
            || response.chunk().await.map_err(|_| unavailable())?.is_some()
        {
            return Err(unavailable());
        }
        Ok(allowed)
    }
}
