use crate::{HttpError, HttpErrorKind as K, JsonDocument, endpoint::Endpoint};
use edgeagent_market_data_application::{
    AccessScope, AuthorizedAcquisition, Cancellation, KnowledgeMode, ProviderFailure as P,
    RequestBudget,
};
use edgeagent_market_data_domain::UtcInstant;
use reqwest::{Client, header};
use std::{num::NonZeroU32, sync::Arc, time::Duration};
use tokio::time::Instant;

/// Explicit reviewed binding and timing controls; no default endpoint, token or interval.
pub struct HttpConfig<'a> {
    /// Exact reviewed principal/account/source/instrument/endpoint/audience/storage scope.
    pub scope: AccessScope,
    /// Trusted HTTPS endpoint including immutable scope parameters, never a command-supplied URL.
    pub endpoint: &'a str,
    /// Sole query key that pagination may add/change; None disables pagination.
    pub cursor_key: Option<&'a str>,
    /// Optional data-only bearer credential, sent as a sensitive header, never in a URL.
    pub bearer: Option<&'a str>,
    /// Explicit positive connect ceiling, at most 5000 ms; must match the admitted budget.
    pub connect_ms: NonZeroU32,
    /// Explicit positive cancellation polling interval, at most 100 ms.
    pub cancellation_poll_ms: NonZeroU32,
}
/// Reusable verified-TLS GET client; no proxy, redirect, retry, cookie or decompression authority.
pub struct HttpTransport {
    client: Client,
    endpoint: Endpoint,
    scope: AccessScope,
    connect_ms: NonZeroU32,
    poll: Duration,
    authorization: Option<header::HeaderValue>,
}
/// Local conservative consumption only; durable accounting belongs to the future owner.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HttpUsage {
    /// Attempts reserved before polling I/O, including failed or uncertain attempts.
    pub attempts: u32,
    /// Identity-encoded bytes observed; may include one rejected overflow chunk.
    pub observed_bytes: u64,
}
/// Non-cloneable single traversal, anchored to the original command's remaining deadline.
pub struct HttpSession<'a> {
    transport: &'a HttpTransport,
    budget: RequestBudget,
    started: Instant,
    deadline: Instant,
    trusted_at: UtcInstant,
    usage: HttpUsage,
}
macro_rules! redacted { ($($t:ty),*) => {$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.write_str(concat!(stringify!($t),"(..)")) }
})*}; }
redacted!(HttpConfig<'_>, HttpTransport, HttpSession<'_>);
impl HttpTransport {
    /// Build a data-only HTTPS client with certificate and hostname verification.
    /// # Errors
    /// Rejects invalid bindings/credentials/limits or an unavailable native trust store.
    pub fn new(config: HttpConfig<'_>) -> Result<Self, HttpError> {
        Self::build(config, false)
    }
    #[cfg(test)]
    pub(crate) fn for_loopback(config: HttpConfig<'_>) -> Result<Self, HttpError> {
        Self::build(config, true)
    }
    fn build(config: HttpConfig<'_>, loopback: bool) -> Result<Self, HttpError> {
        if config.connect_ms.get() > 5000 || config.cancellation_poll_ms.get() > 100 {
            return Err(K::Configuration.into());
        }
        let endpoint = Endpoint::new(config.endpoint, config.cursor_key, loopback)?;
        let authorization = config
            .bearer
            .map(|token| {
                if token.is_empty()
                    || token.len() > 4096
                    || !token.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err(K::Configuration);
                }
                let mut header = header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| K::Configuration)?;
                header.set_sensitive(true);
                Ok(header)
            })
            .transpose()?;
        // Pin the existing ring/native-root stack rather than installing a process-global provider.
        let native = rustls_native_certs::load_native_certs();
        let mut roots = rustls::RootCertStore::empty();
        // Native stores can contain certificates unsupported as TLS trust anchors.
        // Ignoring those only narrows trust; an empty usable store still fails closed.
        roots.add_parsable_certificates(native.certs);
        if roots.is_empty() {
            if let Some(error) = native.errors.into_iter().next() {
                return Err(HttpError::trust(error));
            }
            return Err(K::Configuration.into());
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| K::Configuration)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!loopback)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .tls_sslkeylogfile(false)
            .connection_verbose(false)
            .http1_only()
            .http1_max_headers(32)
            .connect_timeout(Duration::from_millis(config.connect_ms.get().into()))
            .build()
            .map_err(HttpError::request)?;
        Ok(Self {
            client,
            endpoint,
            scope: config.scope,
            connect_ms: config.connect_ms,
            poll: Duration::from_millis(config.cancellation_poll_ms.get().into()),
            authorization,
        })
    }
    /// Start one local traversal from admitted *remaining* budgets and a trusted UTC clock sample.
    /// Recreating sessions cannot replace durable budgets or authorize historical fresh fetches.
    /// # Errors
    /// Rejects scope/knowledge changes, connect-budget mismatch and expired command/retention.
    pub fn start(
        &self,
        admitted: &AuthorizedAcquisition<'_>,
        now: UtcInstant,
    ) -> Result<HttpSession<'_>, HttpError> {
        let request = admitted.request();
        if request.scope != self.scope || request.knowledge != KnowledgeMode::Capture {
            return Err(K::Authority.into());
        }
        if request.budget.connect_ms != self.connect_ms || now < request.admitted_at {
            return Err(K::Configuration.into());
        }
        let until = request.deadline.min(admitted.delete_by()).unix_millis();
        let remaining = until
            .checked_sub(now.unix_millis())
            .filter(|n| *n > 0)
            .ok_or(K::Deadline)?;
        let started = Instant::now();
        Ok(HttpSession {
            transport: self,
            budget: request.budget,
            started,
            deadline: started + Duration::from_millis(remaining as u64),
            trusted_at: now,
            usage: HttpUsage::default(),
        })
    }
}
impl HttpSession<'_> {
    /// Local attempt/observed-byte counters, also available after errors/cancellation.
    #[must_use]
    pub const fn usage(&self) -> HttpUsage {
        self.usage
    }
    /// Fetch exactly one page; None is the configured first URL, Some is an untrusted cursor link.
    /// No automatic retry, pagination, sleep or provider mapping occurs.
    /// Dropping this future stops local polling but does not roll back its reserved attempt.
    /// # Errors
    /// Fails closed on authority expansion, budgets, deadline, cancellation, status or invalid JSON.
    pub async fn get(
        &mut self,
        next: Option<&str>,
        cancellation: &Cancellation,
    ) -> Result<JsonDocument, HttpError> {
        if cancellation.is_cancelled() {
            return Err(K::Cancelled.into());
        }
        if Instant::now() >= self.deadline {
            return Err(K::Deadline.into());
        }
        let url = self.transport.endpoint.page(next)?;
        let attempts = self.budget.calls.get().min(self.budget.pages.get());
        if self.usage.attempts >= attempts
            || self.usage.observed_bytes >= self.budget.command_bytes.get().into()
        {
            return Err(K::Budget.into());
        }
        self.usage.attempts += 1;
        let end = self
            .deadline
            .min(Instant::now() + Duration::from_millis(self.budget.request_ms.get().into()));
        let poll = self.transport.poll;
        let cancelled = async {
            loop {
                if cancellation.is_cancelled() {
                    break;
                }
                tokio::time::sleep(poll).await;
            }
        };
        let result = tokio::select! { biased;
            () = cancelled => Err(K::Cancelled.into()),
            result = tokio::time::timeout_at(end, self.read(url)) => match result {
                Ok(result) => result,
                Err(_) if Instant::now() >= self.deadline => Err(K::Deadline.into()),
                Err(_) => Err(K::Provider(P::Timeout).into()),
            }
        };
        if cancellation.is_cancelled() {
            return Err(K::Cancelled.into());
        }
        if Instant::now() >= self.deadline {
            return Err(K::Deadline.into());
        }
        if Instant::now() >= end {
            return Err(K::Provider(P::Timeout).into());
        }
        result
    }
    async fn read(&mut self, url: url::Url) -> Result<JsonDocument, HttpError> {
        let mut request = self
            .transport
            .client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .header(header::ACCEPT_ENCODING, "identity");
        if let Some(token) = &self.transport.authorization {
            request = request.header(header::AUTHORIZATION, token.clone());
        }
        let mut response = request.send().await.map_err(HttpError::request)?;
        let status = response.status().as_u16();
        if matches!(status, 401 | 403) {
            return Err(K::Provider(P::Forbidden).into());
        }
        if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) {
            let mut retry = response.headers().get_all(header::RETRY_AFTER).iter();
            let retry_after_ms = retry.next().map(|h| self.retry_after(h)).transpose()?;
            if retry.next().is_some() {
                return Err(K::Provider(P::InvalidResponse).into());
            }
            return Err(K::Provider(P::Transient { retry_after_ms }).into());
        }
        if status != 200 {
            return Err(K::Provider(P::InvalidResponse).into());
        }
        let headers = response.headers();
        if headers.get_all(header::CONTENT_TYPE).iter().count() != 1 {
            return Err(K::Provider(P::InvalidResponse).into());
        }
        let mut encoding = headers.get_all(header::CONTENT_ENCODING).iter();
        if encoding
            .next()
            .is_some_and(|v| !v.as_bytes().eq_ignore_ascii_case(b"identity"))
            || encoding.next().is_some()
        {
            return Err(K::Provider(P::InvalidResponse).into());
        }
        let content = headers
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .ok_or(K::Provider(P::InvalidResponse))?;
        let mut parts = content.split(';');
        if !parts
            .next()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            || parts.any(|p| !p.trim().eq_ignore_ascii_case("charset=utf-8"))
        {
            return Err(K::Provider(P::InvalidResponse).into());
        }
        let remaining =
            u64::from(self.budget.command_bytes.get()).saturating_sub(self.usage.observed_bytes);
        let cap = u64::from(self.budget.response_bytes.get()).min(remaining);
        if response.content_length().is_some_and(|len| len > cap) {
            return Err(K::Budget.into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(HttpError::request)? {
            self.usage.observed_bytes += chunk.len() as u64;
            if chunk.len() as u64 > cap.saturating_sub(body.len() as u64) {
                return Err(K::Budget.into());
            }
            body.extend_from_slice(&chunk);
        }
        JsonDocument::from_body(body)
    }
    fn retry_after(&self, header: &header::HeaderValue) -> Result<u32, HttpError> {
        let value = header
            .to_str()
            .map_err(|_| K::Provider(P::InvalidResponse))?;
        let elapsed = self.started.elapsed();
        let delay = if value.bytes().all(|b| b.is_ascii_digit()) && !value.is_empty() {
            Duration::from_secs(value.parse().map_err(|_| K::Budget)?)
        } else {
            let date =
                httpdate::parse_http_date(value).map_err(|_| K::Provider(P::InvalidResponse))?;
            let initial = self.trusted_at.unix_millis();
            let trusted = if initial >= 0 {
                std::time::UNIX_EPOCH.checked_add(Duration::from_millis(initial as u64))
            } else {
                std::time::UNIX_EPOCH.checked_sub(Duration::from_millis(initial.unsigned_abs()))
            }
            .ok_or(K::Configuration)?;
            date.duration_since(trusted.checked_add(elapsed).ok_or(K::Configuration)?)
                .unwrap_or_default()
        };
        if delay >= self.deadline.saturating_duration_since(Instant::now()) {
            return Err(K::Budget.into());
        }
        // Round a fractional HTTP-date delay upwards; never retry before the provider's boundary.
        u32::try_from(delay.as_millis() + u128::from(delay.subsec_nanos() % 1_000_000 != 0))
            .map_err(|_| K::Budget.into())
    }
}
