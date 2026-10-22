use edgeagent_market_data_application::ProviderFailure;

/// Stable payload-free transport rejection categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HttpErrorKind {
    /// Invalid endpoint, credentials, polling interval or resource configuration.
    Configuration,
    /// Attempted scope, origin, path, query or knowledge expansion.
    Authority,
    /// Local call/page/byte budget exhausted, including an unfulfillable Retry-After.
    Budget,
    /// Original command or retention deadline reached.
    Deadline,
    /// Caller requested cancellation; remote work may already have occurred.
    Cancelled,
    /// Invalid, duplicate-key, excessively nested or non-exact JSON data.
    InvalidJson,
    /// Classified request/status failure; never an automatic retry instruction.
    Provider(ProviderFailure),
}
/// Redacted failure; concrete request causes have their URL removed before preservation.
pub struct HttpError {
    pub(crate) kind: HttpErrorKind,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl HttpError {
    /// Return the stable category without exposing request metadata or payloads.
    #[must_use]
    pub const fn kind(&self) -> HttpErrorKind {
        self.kind
    }
    pub(crate) const fn new(kind: HttpErrorKind) -> Self {
        Self { kind, source: None }
    }
    pub(crate) fn request(error: reqwest::Error) -> Self {
        let failure = if error.is_timeout() {
            ProviderFailure::Timeout
        } else if error.is_connect() {
            ProviderFailure::Transient {
                retry_after_ms: None,
            }
        } else {
            ProviderFailure::InvalidResponse
        };
        Self {
            kind: HttpErrorKind::Provider(failure),
            source: Some(Box::new(error.without_url())),
        }
    }
    pub(crate) fn trust(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind: HttpErrorKind::Configuration,
            source: Some(Box::new(error)),
        }
    }
}
impl From<HttpErrorKind> for HttpError {
    fn from(kind: HttpErrorKind) -> Self {
        Self::new(kind)
    }
}
impl std::fmt::Debug for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HttpError").field(&self.kind).finish()
    }
}
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "provider HTTP rejected: {:?}", self.kind)
    }
}
impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|e| e as _)
    }
}
