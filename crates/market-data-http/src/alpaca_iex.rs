//! Offline-qualified IEX protocol. No runtime selector, retries or live activation.
mod protocol;
#[cfg(test)]
mod tests;
use crate::{WssConfig, WssErrorKind, WssFrameKind, WssSession, WssTransport};
use edgeagent_market_data_application::{
    Cancellation, SelectionPlan, StreamingAuthority, StreamingLedger, StreamingSession,
};
use edgeagent_market_data_domain::*;
use protocol::{Decoder, State};
use std::num::NonZeroU32;

/// Payload-free provider protocol failures; none authorizes reconnection or slot release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IexErrorKind {
    /// Invalid finite configuration or incompatible session/selection/feed contract.
    Configuration,
    /// Malformed framing, unexpected state, payload or subscription acknowledgement.
    Protocol,
    /// Provider rejected authentication, entitlement, capacity or processing.
    Provider,
    /// Correction/cancel requires reconciliation, never silent immutable-data replacement.
    Reconciliation,
    /// Explicit assembly, batch or protocol deadline exhausted.
    Budget,
    /// This local handle already failed or an operation was abandoned.
    Stopped,
    /// Preserve the bounded transport's distinct accounting/authority/cancellation outcomes.
    Transport(WssErrorKind),
}
/// Redacted error with no credential, symbol, provider body or untrusted message.
pub struct IexError {
    kind: IexErrorKind,
    source: Option<crate::WssError>,
}
impl IexError {
    /// Stable classification without exposing licensed or sensitive input.
    #[must_use]
    pub const fn kind(&self) -> IexErrorKind {
        self.kind
    }
}
impl From<IexErrorKind> for IexError {
    fn from(k: IexErrorKind) -> Self {
        Self {
            kind: k,
            source: None,
        }
    }
}
impl From<crate::WssError> for IexError {
    fn from(e: crate::WssError) -> Self {
        Self {
            kind: IexErrorKind::Transport(e.kind()),
            source: Some(e),
        }
    }
}
impl std::fmt::Display for IexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IEX rejected: {:?}", self.kind)
    }
}
impl std::fmt::Debug for IexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IexError").field(&self.kind).finish()
    }
}
impl std::error::Error for IexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|source| source as _)
    }
}
type Result<T> = std::result::Result<T, IexError>;

/// Explicit finite data-only configuration; construction performs no provider calls.
pub struct AlpacaIexConfig<'a> {
    /// Existing bounded WSS allocation, fixed to the IEX endpoint, without bearer auth.
    pub transport: WssConfig<'a>,
    /// Owner-supplied data API key ID; no environment or credential discovery.
    pub key_id: &'a str,
    /// Owner-supplied secret; sent only as JSON over verified TLS, never diagnostics.
    pub secret: &'a str,
    /// Entire welcome/authentication/subscription deadline, 1..=10000 ms after upgrade.
    pub handshake_ms: NonZeroU32,
    /// Maximum records per complete JSON message, 1..=1024.
    pub batch_records: NonZeroU32,
}
/// Fixed IEX adapter library, not an activated daemon or a market-data rights grant.
pub struct AlpacaIex {
    transport: WssTransport,
    key_id: String,
    secret: String,
    handshake_ms: u32,
    batch_records: u32,
    message_bytes: usize,
}
/// Typed provider value, still requiring owner-authenticated semantic/identity acceptance.
#[non_exhaustive]
#[derive(Clone)]
pub enum IexValue {
    /// Positive exact trade; provider ID is not a sequence or globally unique identity.
    Trade {
        /// Provider trade identifier, scoped to source/instrument/exchange/session.
        id: u64,
        /// Structurally checked exact USD/share and positive shares.
        value: ShareTrade,
        /// Original provider exchange code, not an inferred listing venue.
        exchange: String,
        /// Original condition codes, not a guessed canonical eligibility mapping.
        conditions: Vec<String>,
        /// Original tape code, not source continuity.
        tape: String,
    },
    /// Original minute aggregate; later updates require reconciliation.
    MinuteBar {
        /// Checked OHLCV and exact minute bucket, including extended-session coverage.
        value: RawBar,
        /// Reported trade count, not a sequence.
        trades: u64,
        /// Exact provider volume-weighted price, not recomputed from missing trades.
        vwap: UsdPrice,
    },
}
/// Transient normalized record; not a canonical observation, publication or readiness proof.
#[derive(Clone)]
pub struct IexObservation {
    /// Exact mapping from the admitted frozen selection, preserving its revision.
    pub mapping: ReferenceVersion<InstrumentMapping>,
    /// Explicit IEX contract, never historical SIP or consolidated coverage.
    pub contract: EvidenceContract,
    /// Original provider event time at nanosecond precision.
    pub event_at: PreciseInstant,
    /// Complete-message receipt from the final confirmed frame; not provider time.
    pub received_at: UtcInstant,
    /// Inclusive charged local frame range spanning this message, possibly interleaved controls.
    pub frames: [u64; 2],
    /// Original item position before selection filtering.
    pub item: u32,
    /// Exact provider value; raw conditions require reviewed semantic mapping in PR20.
    pub value: IexValue,
}
/// One bounded intake result, not an unbounded background feed.
#[non_exhaustive]
pub enum IexProgress {
    /// Welcome/authentication/subscription control advanced; no market freshness implied.
    Control,
    /// Exact subscription acknowledgement; does not establish evidence readiness.
    Subscribed,
    /// Entire message validated before any selected record is returned; may be empty.
    Records(Vec<IexObservation>),
}
/// Single-owner protocol/socket; abandoned `next` leaves no reusable socket.
pub struct AlpacaIexSession<'a, L, A> {
    adapter: &'a AlpacaIex,
    socket: Option<WssSession<'a, L, A>>,
    decoder: Decoder<'a>,
    handshake_deadline: tokio::time::Instant,
    fragment: Option<(u64, Vec<u8>)>,
}
macro_rules! redact {($($t:ty),*)=>{$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*};}
redact!(
    AlpacaIexConfig<'_>,
    AlpacaIex,
    IexValue,
    IexObservation,
    IexProgress
);
impl<L, A> std::fmt::Debug for AlpacaIexSession<'_, L, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AlpacaIexSession(..)")
    }
}
impl AlpacaIex {
    /// Build the fixed verified-TLS IEX library. Never selects a runtime or contacts a provider.
    /// # Errors
    /// Rejects absent/oversized credentials, another feed, bearer auth or unbounded limits.
    pub fn new(config: AlpacaIexConfig<'_>) -> Result<Self> {
        Self::validate(&config)?;
        let message_bytes = config.transport.message_bytes.get() as usize;
        let transport = WssTransport::new(config.transport)?;
        Ok(Self {
            transport,
            key_id: config.key_id.to_owned(),
            secret: config.secret.to_owned(),
            handshake_ms: config.handshake_ms.get(),
            batch_records: config.batch_records.get(),
            message_bytes,
        })
    }
    fn validate(config: &AlpacaIexConfig<'_>) -> Result<()> {
        if config.transport.endpoint != "wss://stream.data.alpaca.markets/v2/iex"
            || config.transport.bearer.is_some()
            || config.transport.message_bytes.get() > 1_048_576
            || config.handshake_ms.get() > 10_000
            || config.batch_records.get() > 1024
            || [config.key_id, config.secret]
                .iter()
                .any(|s| s.is_empty() || s.len() > 512 || !s.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err(IexErrorKind::Configuration.into());
        }
        Ok(())
    }
    /// Bind one frozen trade OR minute-bar contract, then reserve and connect once.
    /// Trades and bars use separate admissions; no multi-contract session is invented here.
    /// # Errors
    /// Rejects incompatible scope/precision/delivery/selection before dialing. No retries.
    pub async fn connect<'a, L: StreamingLedger, A: StreamingAuthority>(
        &'a self,
        session: &'a StreamingSession,
        selection: &'a SelectionPlan<'a>,
        ledger: &'a L,
        authority: &'a A,
        cancellation: &Cancellation,
    ) -> Result<AlpacaIexSession<'a, L, A>> {
        let decoder = Decoder::new(session, selection, self.batch_records)?;
        let socket = self
            .transport
            .connect(session, ledger, authority, cancellation)
            .await?;
        Ok(AlpacaIexSession {
            adapter: self,
            socket: Some(socket),
            decoder,
            handshake_deadline: tokio::time::Instant::now()
                + std::time::Duration::from_millis(self.handshake_ms.into()),
            fragment: None,
        })
    }
}
impl<L: StreamingLedger, A: StreamingAuthority> AlpacaIexSession<'_, L, A> {
    /// Exact locally reserved generation for future capture composition, not quiescence/authority.
    /// A stopped/abandoned socket exposes no reusable connection handle.
    #[must_use]
    pub fn connection(&self) -> Option<&edgeagent_market_data_application::StreamingConnection> {
        self.socket.as_ref().map(WssSession::connection)
    }
    /// Receive one bounded message/control. Any error or abandoned future stops local reuse.
    /// # Errors
    /// Preserves transport errors; rejects timeout, malformed batches and unresolved corrections.
    pub async fn next(&mut self, cancellation: &Cancellation) -> Result<IexProgress> {
        let mut socket = self.socket.take().ok_or(IexErrorKind::Stopped)?;
        let deadline = self.handshake_deadline;
        let handshaking = self.decoder.state != State::Streaming;
        if handshaking && tokio::time::Instant::now() >= deadline {
            self.fragment = None;
            self.decoder.state = State::Stopped;
            return Err(IexErrorKind::Budget.into());
        }
        let future = self.read(&mut socket, cancellation);
        let result = if handshaking {
            tokio::time::timeout_at(deadline, future)
                .await
                .map_err(|_| IexError::from(IexErrorKind::Budget))
                .and_then(|r| r)
        } else {
            future.await
        };
        // Never replace an accounting/authority failure with a clock observation.
        let result = if result.is_ok() && handshaking && tokio::time::Instant::now() >= deadline {
            Err(IexErrorKind::Budget.into())
        } else {
            result
        };
        match result {
            Ok(progress) => {
                self.socket = Some(socket);
                Ok(progress)
            }
            Err(error) => {
                self.fragment = None;
                self.decoder.state = State::Stopped;
                Err(error)
            }
        }
    }
    async fn read(
        &mut self,
        socket: &mut WssSession<'_, L, A>,
        cancellation: &Cancellation,
    ) -> Result<IexProgress> {
        loop {
            let frame = socket.receive(cancellation).await?;
            match frame.kind {
                WssFrameKind::Ping => {
                    socket
                        .send(WssFrameKind::Pong, frame.payload(), cancellation)
                        .await?;
                    return Ok(IexProgress::Control);
                }
                WssFrameKind::Pong => return Ok(IexProgress::Control),
                WssFrameKind::Close | WssFrameKind::Binary => {
                    return Err(IexErrorKind::Protocol.into());
                }
                WssFrameKind::Text => {
                    if self.fragment.is_some() {
                        return Err(IexErrorKind::Protocol.into());
                    }
                    self.fragment = Some((frame.ordinal, Vec::new()));
                }
                WssFrameKind::Continuation => {}
            }
            let (first, bytes) = self.fragment.as_mut().ok_or(IexErrorKind::Protocol)?;
            if bytes
                .len()
                .checked_add(frame.payload().len())
                .is_none_or(|n| n > self.adapter.message_bytes)
            {
                return Err(IexErrorKind::Budget.into());
            }
            bytes.extend_from_slice(frame.payload());
            if !frame.final_fragment {
                continue;
            }
            let range = [*first, frame.ordinal];
            let (_, bytes) = self.fragment.take().ok_or(IexErrorKind::Protocol)?;
            let progress = self.decoder.decode(&bytes, range, frame.received_at)?;
            let outgoing = match self.decoder.state {
                State::Authenticate => {
                    let message = serde_json::json!({
                        "action": "auth",
                        "key": self.adapter.key_id,
                        "secret": self.adapter.secret,
                    });
                    Some(serde_json::to_vec(&message).map_err(|_| IexErrorKind::Protocol)?)
                }
                State::Subscribe => Some(self.decoder.subscription()?),
                _ => None,
            };
            if let Some(bytes) = outgoing {
                socket
                    .send(WssFrameKind::Text, &bytes, cancellation)
                    .await?;
            }
            return Ok(progress);
        }
    }
}
