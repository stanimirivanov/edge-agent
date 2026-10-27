//! Frame-level WSS, isolated from provider mapping and evidence readiness.
mod connection;
mod session;
use session::guard;
#[cfg(test)]
mod tests;
use edgeagent_market_data_application::{
    AccessScope, BudgetError, Cancellation, StreamingArrival, StreamingAuthority,
    StreamingConnection, StreamingLedger, StreamingSession,
};
use fastwebsockets::{Frame, OpCode, Payload, WebSocket};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use std::{future::Future, num::NonZeroU32, sync::Arc, time::Duration};
use tokio::time::Instant;
use url::Url;
type Socket = WebSocket<TokioIo<Upgraded>>;
type Result<T> = std::result::Result<T, WssError>;

/// Stable payload-free failure categories; no failure authorizes an automatic retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WssErrorKind {
    /// Invalid endpoint, credentials or explicit allocation limits.
    Configuration,
    /// Current authority or generation denied the operation.
    Authority,
    /// Durable state is unavailable or unresolved; a commit may have occurred.
    Accounting,
    /// Original session deadline or operation timeout expired.
    Timeout,
    /// Cooperative cancellation, not proof of provider quiescence.
    Cancelled,
    /// Incoming/outgoing work allocation exhausted.
    Budget,
    /// TLS, HTTP upgrade, framing or socket failure.
    Protocol,
    /// Stopped handle or previously abandoned operation future.
    Stopped,
}
/// Redacted error. Explicit source inspection is privileged; never blindly log its chain.
pub struct WssError {
    kind: WssErrorKind,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl WssError {
    /// Stable category without payload or endpoint metadata.
    #[must_use]
    pub const fn kind(&self) -> WssErrorKind {
        self.kind
    }
    fn caused(kind: WssErrorKind, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }
}
impl From<WssErrorKind> for WssError {
    fn from(kind: WssErrorKind) -> Self {
        Self { kind, source: None }
    }
}
impl From<BudgetError> for WssError {
    fn from(e: BudgetError) -> Self {
        use edgeagent_market_data_application::BudgetErrorKind as B;
        let kind = match e.kind() {
            B::Storage | B::Busy => WssErrorKind::Accounting,
            B::Cancelled => WssErrorKind::Cancelled,
            B::Deadline => WssErrorKind::Timeout,
            B::Exhausted | B::RateLimited => WssErrorKind::Budget,
            _ => WssErrorKind::Authority,
        };
        Self::caused(kind, e)
    }
}
impl std::fmt::Debug for WssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WssError").field(&self.kind).finish()
    }
}
impl std::fmt::Display for WssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WSS rejected: {:?}", self.kind)
    }
}
impl std::error::Error for WssError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|s| s as _)
    }
}

/// Explicit owner-reviewed controls, not a message-supplied URL or credential.
pub struct WssConfig<'a> {
    /// Exact admitted binding; trusted composition resolves its endpoint ID to the URL.
    pub scope: AccessScope,
    /// WSS only: no query, userinfo, fragment, redirect or proxy.
    pub endpoint: &'a str,
    /// Optional data-only bearer, sent solely as a sensitive header.
    pub bearer: Option<&'a str>,
    /// Positive cancellation/authority polling interval, at most 100 ms.
    pub poll_ms: NonZeroU32,
    /// Upgrade receive-buffer ceiling, 8192..=65536 bytes.
    pub upgrade_bytes: NonZeroU32,
    /// Aggregate fragmented-message ceiling, at most 4 MiB; frames remain unassembled.
    pub message_bytes: NonZeroU32,
    /// Maximum attempted outgoing frames, including required protocol replies.
    pub writes: NonZeroU32,
    /// Aggregate outgoing payload budget, at most 4 MiB.
    pub write_bytes: NonZeroU32,
}
/// Verified TLS transport. Construction never dials or grants activation.
pub struct WssTransport {
    config: OwnedConfig,
    tls: Arc<rustls::ClientConfig>,
}
struct OwnedConfig {
    scope: AccessScope,
    url: Url,
    authorization: Option<hyper::header::HeaderValue>,
    poll: Duration,
    upgrade_bytes: usize,
    message_bytes: u32,
    writes: u32,
    write_bytes: u32,
}
/// Raw charged frame kind. Continuations are not complete messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WssFrameKind {
    /// First text fragment. Provider decoder validates complete-message UTF-8.
    Text,
    /// First binary fragment.
    Binary,
    /// Continuation of the current bounded fragmented message.
    Continuation,
    /// Charged ping. Send the exact bounded pong before receiving again.
    Ping,
    /// Pong, never market freshness evidence.
    Pong,
    /// Peer close; local handle cannot be reused.
    Close,
}
/// Owned licensed frame bytes, transient and payload-redacted.
pub struct WssFrame {
    /// Exact confirmed session-local arrival ordinal.
    pub ordinal: u64,
    /// Trusted complete-frame receipt before accounting, not provider time or freshness.
    pub received_at: edgeagent_market_data_domain::UtcInstant,
    /// Raw frame category.
    pub kind: WssFrameKind,
    /// Whether this is the final fragment.
    pub final_fragment: bool,
    payload: Vec<u8>,
}
impl WssFrame {
    /// Borrow transient bytes; no evidence acceptance or persistence permission is implied.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}
/// Single-owner socket bound to a durable generation. Drop releases no account capacity.
/// Abandoning an operation consumes the socket; never resume a partially parsed stream.
pub struct WssSession<'a, L, A> {
    transport: &'a WssTransport,
    ledger: &'a L,
    authority: &'a A,
    session: &'a StreamingSession,
    connection: StreamingConnection,
    socket: Option<Socket>,
    deadline: Instant,
    idle_deadline: Instant,
    last_utc: i64,
    ordinal: u64,
    fragment_bytes: Option<u32>,
    writes: u32,
    written: u32,
    pending_pong: Option<Vec<u8>>,
}
macro_rules! redacted {($($t:ty),*)=>{$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*};}
redacted!(WssConfig<'_>, WssTransport, WssFrame);
impl<L, A> std::fmt::Debug for WssSession<'_, L, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WssSession(..)")
    }
}
impl WssTransport {
    #[cfg(test)]
    pub(crate) fn for_test(config: WssConfig<'_>, roots: rustls::RootCertStore) -> Result<Self> {
        Self::build(config, roots)
    }
    /// Use verified native trust roots and the existing ring TLS provider; no global mutation.
    /// # Errors
    /// Rejects invalid configuration or an empty usable trust store.
    pub fn new(config: WssConfig<'_>) -> Result<Self> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
        Self::build(config, roots)
    }
    fn build(config: WssConfig<'_>, roots: rustls::RootCertStore) -> Result<Self> {
        if config.endpoint.len() > 4096 {
            return Err(WssErrorKind::Configuration.into());
        }
        let url = Url::parse(config.endpoint).map_err(|_| WssErrorKind::Configuration)?;
        if url.scheme() != "wss"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || config.poll_ms.get() > 100
            || roots.is_empty()
            || !(8192..=65536).contains(&config.upgrade_bytes.get())
            || config.message_bytes.get() > 4 * 1024 * 1024
            || config.write_bytes.get() > 4 * 1024 * 1024
        {
            return Err(WssErrorKind::Configuration.into());
        }
        let authorization = config
            .bearer
            .map(|value| {
                if value.is_empty()
                    || value.len() > 4096
                    || !value.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err(WssErrorKind::Configuration);
                }
                let mut header = hyper::header::HeaderValue::from_str(&format!("Bearer {value}"))
                    .map_err(|_| WssErrorKind::Configuration)?;
                header.set_sensitive(true);
                Ok(header)
            })
            .transpose()?;
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| WssErrorKind::Configuration)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(Self {
            config: OwnedConfig {
                scope: config.scope,
                url,
                authorization,
                poll: Duration::from_millis(config.poll_ms.get().into()),
                upgrade_bytes: config.upgrade_bytes.get() as usize,
                message_bytes: config.message_bytes.get(),
                writes: config.writes.get(),
                write_bytes: config.write_bytes.get(),
            },
            tls: Arc::new(tls),
        })
    }
    /// Commit one new connection/start reservation, then attempt TCP/TLS/upgrade once.
    /// # Errors
    /// Scope/authority fail before reserve. Failed, cancelled or unknown starts never refund slots.
    pub async fn connect<'a, L: StreamingLedger, A: StreamingAuthority>(
        &'a self,
        session: &'a StreamingSession,
        ledger: &'a L,
        authority: &'a A,
        cancellation: &Cancellation,
    ) -> Result<WssSession<'a, L, A>> {
        let now = authority.now();
        session.recheck(authority.catalogs(), now, cancellation)?;
        if !session.data().scopes.contains(&self.config.scope)
            || session.data().limits.frame_bytes.get() > self.config.message_bytes
        {
            return Err(WssErrorKind::Configuration.into());
        }
        let remaining = session.data().window.bounds().1.unix_millis() - now.unix_millis();
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(remaining as u64))
            .ok_or(WssErrorKind::Configuration)?;
        let mut last_utc = now.unix_millis();
        let connection = guard(
            authority,
            session,
            &mut last_utc,
            deadline,
            self.config.poll,
            cancellation,
            async {
                Ok(ledger
                    .reserve(session, authority.catalogs(), cancellation)
                    .await?)
            },
        )
        .await?;
        if !connection.matches(session) || connection.usage().stopped {
            return Err(WssErrorKind::Authority.into());
        }
        let ordinal = connection.usage().frames;
        let until = deadline.min(
            Instant::now() + Duration::from_millis(session.data().limits.connect_ms.get().into()),
        );
        let socket = guard(
            authority,
            session,
            &mut last_utc,
            until,
            self.config.poll,
            cancellation,
            connection::open(self, session),
        )
        .await?;
        Ok(WssSession {
            transport: self,
            ledger,
            authority,
            session,
            connection,
            socket: Some(socket),
            deadline,
            idle_deadline: Instant::now()
                + Duration::from_millis(session.data().limits.idle_ms.get().into()),
            last_utc,
            ordinal,
            fragment_bytes: None,
            writes: 0,
            written: 0,
            pending_pong: None,
        })
    }
}
