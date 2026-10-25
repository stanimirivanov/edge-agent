use crate::{HttpError, HttpErrorKind as K, JsonDocument, endpoint::Endpoint};
#[cfg(test)]
use edgeagent_market_data_application::AuthorizedAcquisition;
use edgeagent_market_data_application::{
    AccessScope, AcquisitionRequest, Cancellation, DispatchCheck, KnowledgeMode,
    ProviderFailure as P, RequestBudget,
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
    data_headers: header::HeaderMap,
}
/// Local conservative observations; `MeteredSession` projects authoritative ledger usage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HttpUsage {
    /// Attempts reserved before polling I/O, including failed or uncertain attempts.
    pub attempts: u32,
    /// Identity-encoded bytes observed; may include one rejected overflow chunk.
    pub observed_bytes: u64,
    /// Last attempt reached complete JSON-body receipt or a classified terminal HTTP status.
    pub response_observed: bool,
    /// Last valid provider delay, preserved even when it exceeds the current command deadline.
    pub retry_after_ms: Option<u32>,
}
/// Non-cloneable single traversal, anchored to the original command's remaining deadline.
pub(crate) struct HttpSession<'a> {
    transport: &'a HttpTransport,
    budget: RequestBudget,
    started: Instant,
    deadline: Instant,
    trusted_at: UtcInstant,
    last_checked_at: UtcInstant,
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
        Self::build(config, false, "Bearer")
    }
    #[cfg(test)]
    pub(crate) fn for_loopback(config: HttpConfig<'_>) -> Result<Self, HttpError> {
        Self::build(config, true, "Bearer")
    }
    pub(crate) fn api_key(config: HttpConfig<'_>, loopback: bool) -> Result<Self, HttpError> {
        Self::build(config, loopback, "apikey")
    }
    pub(crate) fn alpaca(
        config: HttpConfig<'_>,
        id: &str,
        secret: &str,
        loopback: bool,
    ) -> Result<Self, HttpError> {
        if config.bearer.is_some() {
            return Err(K::Configuration.into());
        }
        let mut client = Self::build(config, loopback, "Bearer")?;
        for (name, value) in [("apca-api-key-id", id), ("apca-api-secret-key", secret)] {
            if value.is_empty()
                || value.len() > 4096
                || !value.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(K::Configuration.into());
            }
            let mut header = header::HeaderValue::from_str(value).map_err(|_| K::Configuration)?;
            header.set_sensitive(true);
            client.data_headers.insert(name, header);
        }
        Ok(client)
    }
    fn build(config: HttpConfig<'_>, loopback: bool, scheme: &str) -> Result<Self, HttpError> {
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
                let mut header = header::HeaderValue::from_str(&format!("{scheme} {token}"))
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
            data_headers: header::HeaderMap::new(),
        })
    }
    /// Start one local traversal from admitted *remaining* budgets and a trusted UTC clock sample.
    /// Recreating sessions cannot replace durable budgets or authorize historical fresh fetches.
    /// # Errors
    /// Rejects scope/knowledge changes, connect-budget mismatch and expired command/retention.
    #[cfg(test)]
    pub(crate) fn start(
        &self,
        admitted: &AuthorizedAcquisition<'_>,
        now: UtcInstant,
    ) -> Result<HttpSession<'_>, HttpError> {
        self.start_checked(admitted.request(), now, admitted.delete_by().unix_millis())
    }
    pub(crate) fn start_checked(
        &self,
        request: &AcquisitionRequest,
        now: UtcInstant,
        until: i64,
    ) -> Result<HttpSession<'_>, HttpError> {
        if request.scope != self.scope || request.knowledge != KnowledgeMode::Capture {
            return Err(K::Authority.into());
        }
        if request.budget.connect_ms != self.connect_ms || now < request.admitted_at {
            return Err(K::Configuration.into());
        }
        let until = request.deadline.unix_millis().min(until);
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
            last_checked_at: now,
            usage: HttpUsage::default(),
        })
    }
}
impl HttpSession<'_> {
    pub(crate) fn recheck(&mut self, check: &DispatchCheck) -> Result<(), HttpError> {
        if check.at() < self.last_checked_at {
            return Err(K::Authority.into());
        }
        self.last_checked_at = check.at();
        let remaining = check
            .valid_until_ms()
            .checked_sub(check.at().unix_millis())
            .filter(|ms| *ms > 0)
            .ok_or(K::Deadline)?;
        self.deadline = self
            .deadline
            .min(Instant::now() + Duration::from_millis(remaining as u64));
        if Instant::now() >= self.deadline {
            return Err(K::Deadline.into());
        }
        Ok(())
    }
    pub(crate) fn validate_page(&self, next: Option<&str>) -> Result<(), HttpError> {
        self.transport.endpoint.page(next).map(|_| ())
    }
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
        self.usage.response_observed = false;
        self.usage.retry_after_ms = None;
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
        request = request.headers(self.transport.data_headers.clone());
        if let Some(token) = &self.transport.authorization {
            request = request.header(header::AUTHORIZATION, token.clone());
        }
        let mut response = request.send().await.map_err(HttpError::request)?;
        let status = response.status().as_u16();
        if matches!(status, 401 | 403) {
            self.usage.response_observed = true;
            return Err(K::Provider(P::Forbidden).into());
        }
        if matches!(status, 408 | 429 | 500 | 502 | 503 | 504) {
            let mut retry = response.headers().get_all(header::RETRY_AFTER).iter();
            let retry_after_ms = retry.next().map(|h| self.retry_after(h)).transpose()?;
            if retry.next().is_some() {
                return Err(K::Provider(P::InvalidResponse).into());
            }
            self.usage.retry_after_ms = retry_after_ms;
            self.usage.response_observed = true;
            if retry_after_ms.is_some_and(|ms| {
                Duration::from_millis(ms.into())
                    >= self.deadline.saturating_duration_since(Instant::now())
            }) {
                return Err(K::Budget.into());
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
        self.usage.response_observed = true;
        let received_monotonic = Instant::now();
        let elapsed = i64::try_from(self.started.elapsed().as_millis()).map_err(|_| K::Deadline)?;
        let received = self
            .trusted_at
            .unix_millis()
            .checked_add(elapsed)
            .ok_or(K::Deadline)?;
        let received =
            UtcInstant::from_unix_millis(received.max(self.last_checked_at.unix_millis()))
                .map_err(|_| K::Deadline)?;
        let mut document = JsonDocument::from_body(body)?;
        document.received = Some(received);
        document.received_monotonic = Some(received_monotonic);
        Ok(document)
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
        // Round a fractional HTTP-date delay upwards; never retry before the provider's boundary.
        u32::try_from(delay.as_millis() + u128::from(delay.subsec_nanos() % 1_000_000 != 0))
            .map_err(|_| K::Budget.into())
    }
}
