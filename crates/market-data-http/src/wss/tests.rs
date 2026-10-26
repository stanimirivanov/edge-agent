//! Owned TLS loopback peers only; no DNS provider, vendor, Docker or credentials.
#[allow(dead_code)]
#[path = "../../../market-data-application/tests/support/streaming.rs"]
mod fixture;
use super::*;
use edgeagent_market_data_application::{
    AuthorityCatalogs, BudgetErrorKind, StreamingFrame, StreamingUsage,
};
use fixture::{Fixture, at, nz};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const AT: i64 = 1_800_000_000_000;
struct Authority {
    fixture: Fixture,
    now: edgeagent_market_data_domain::UtcInstant,
    revoked: AtomicBool,
    withdrawn: edgeagent_market_data_domain::ReferenceHistory<
        edgeagent_market_data_application::RightsRecord,
    >,
}
impl Authority {
    fn new() -> TestResult<Self> {
        let fixture = Fixture::new(AT, 1)?;
        let withdrawn = fixture.revoked()?;
        Ok(Self {
            fixture,
            now: at(AT + 1)?,
            revoked: AtomicBool::new(false),
            withdrawn,
        })
    }
}
impl StreamingAuthority for Authority {
    fn now(&self) -> edgeagent_market_data_domain::UtcInstant {
        self.now
    }
    fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: if self.revoked.load(Ordering::SeqCst) {
                &self.withdrawn
            } else {
                &self.fixture.rights
            },
            providers: &self.fixture.providers,
        }
    }
}
struct Ledger {
    usage: Mutex<StreamingUsage>,
    reserves: AtomicUsize,
    lose: AtomicBool,
    stall: AtomicBool,
    charged: tokio::sync::Notify,
}
impl Ledger {
    fn new() -> Self {
        Self {
            usage: Mutex::new(StreamingUsage {
                starts: 1,
                frames: 0,
                bytes: 0,
                last_frame_bytes: None,
                stopped: false,
            }),
            reserves: AtomicUsize::new(0),
            lose: AtomicBool::new(false),
            stall: AtomicBool::new(false),
            charged: tokio::sync::Notify::new(),
        }
    }
}
impl StreamingLedger for Ledger {
    async fn reserve(
        &self,
        s: &StreamingSession,
        _: AuthorityCatalogs<'_>,
        _: &Cancellation,
    ) -> std::result::Result<StreamingConnection, BudgetError> {
        self.reserves.fetch_add(1, Ordering::SeqCst);
        let usage = *self
            .usage
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?;
        StreamingConnection::from_committed(s, 1, usage)
    }
    async fn charge(
        &self,
        s: &StreamingSession,
        _: &StreamingConnection,
        c: AuthorityCatalogs<'_>,
        cancel: &Cancellation,
        a: StreamingArrival,
    ) -> std::result::Result<StreamingFrame, BudgetError> {
        s.recheck(
            c,
            at(AT + 1).map_err(|_| BudgetError::new(BudgetErrorKind::Invalid))?,
            cancel,
        )?;
        let frame = {
            let mut usage = self
                .usage
                .lock()
                .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?;
            let frame = s.data().limits.charge(*usage, a.ordinal, a.bytes)?;
            *usage = frame.usage;
            frame
        };
        self.charged.notify_one();
        if self.stall.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.lose.load(Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Storage));
        }
        Ok(frame)
    }
}
enum Reply {
    Valid,
    InvalidAccept,
    Redirect,
    Extensions,
    Large,
    Silent,
}
struct Peer {
    url: String,
    roots: rustls::RootCertStore,
    request_read: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn peer(reply: Reply, wire: Vec<u8>, hostname_ok: bool) -> TestResult<Peer> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha1::{Digest, Sha1};
    let name = if hostname_ok {
        "127.0.0.1"
    } else {
        "localhost"
    };
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec![name.to_owned()])?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone())?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("wss://{}/stream", listener.local_addr()?);
    let request_read = Arc::new(tokio::sync::Notify::new());
    let ready = request_read.clone();
    let task = tokio::spawn(async move {
        let work = async {
            let (tcp, _) = listener.accept().await?;
            let mut socket = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
                .accept(tcp)
                .await?;
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                request.push(socket.read_u8().await?);
            }
            ready.notify_one();
            if matches!(reply, Reply::Silent) {
                std::future::pending::<()>().await;
            }
            let request = std::str::from_utf8(&request)?;
            let key = request
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
                        .map(|(_, v)| v.trim())
                })
                .ok_or("key")?;
            let mut hash = Sha1::new();
            hash.update(key);
            hash.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
            let accept = if matches!(reply, Reply::InvalidAccept) {
                "invalid".into()
            } else {
                STANDARD.encode(hash.finalize())
            };
            let status = if matches!(reply, Reply::Redirect) {
                "302 Found"
            } else {
                "101 Switching Protocols"
            };
            let extra = if matches!(reply, Reply::Extensions) {
                "Sec-WebSocket-Extensions: permessage-deflate\r\n".into()
            } else if matches!(reply, Reply::Large) {
                format!("X-Long: {}\r\n", "x".repeat(9000))
            } else {
                String::new()
            };
            socket.write_all(format!("HTTP/1.1 {status}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{extra}\r\n").as_bytes()).await?;
            socket.write_all(&wire).await?;
            socket.flush().await?;
            // Wait for client EOF/close without timing-based test coordination.
            let mut bytes = [0; 512];
            while socket.read(&mut bytes).await? != 0 {}
            TestResult::Ok(())
        };
        let _ = work.await;
    });
    Ok(Peer {
        url,
        roots,
        request_read,
        task,
    })
}
// Pure ledger examples use 100 ms; those values are not TLS scheduling assertions.
// Protocol proofs get explicit finite local-I/O budgets. Timing proofs use barriers
// and virtual time so neither busy Windows runners nor retries determine success.
fn session_data(
    a: &Authority,
) -> TestResult<edgeagent_market_data_application::StreamingSessionData> {
    let mut data = a.fixture.data(1)?;
    data.limits.connect_ms = nz(5000)?;
    data.limits.idle_ms = nz(5000)?;
    Ok(data)
}
fn session(a: &Authority) -> TestResult<StreamingSession> {
    a.fixture.seal(session_data(a)?)
}
fn config<'a>(authority: &Authority, endpoint: &'a str) -> TestResult<WssConfig<'a>> {
    Ok(WssConfig {
        scope: *authority.fixture.scopes.first().ok_or("scope")?,
        endpoint,
        bearer: None,
        poll_ms: nz(10)?,
        upgrade_bytes: nz(8192)?,
        message_bytes: nz(20)?,
        writes: nz(2)?,
        write_bytes: nz(20)?,
    })
}
fn transport(authority: &Authority, peer: &Peer) -> TestResult<WssTransport> {
    Ok(WssTransport::build(
        config(authority, &peer.url)?,
        peer.roots.clone(),
    )?)
}

#[tokio::test]
async fn wss_charges_controls_fragments_and_exact_caps_before_exposure() -> TestResult {
    let a = Authority::new()?;
    let l = Ledger::new();
    let s = session(&a)?;
    let cancel = Cancellation::default();
    let peer = peer(
        Reply::Valid,
        vec![0x01, 1, b'a', 0x89, 0, 0x80, 1, b'b'],
        true,
    )
    .await?;
    let t = transport(&a, &peer)?;
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    let first = ws.receive(&cancel).await?;
    assert!(!first.final_fragment);
    assert_eq!(first.received_at, a.now());
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 1);
    assert_eq!(ws.receive(&cancel).await?.kind, WssFrameKind::Ping);
    ws.send(WssFrameKind::Pong, b"", &cancel).await?;
    let frame = ws.receive(&cancel).await?;
    assert_eq!(frame.ordinal, 3);
    assert_eq!(frame.payload(), b"b");
    assert_eq!(frame.kind, WssFrameKind::Continuation);
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Stopped
    );
    assert_eq!(l.reserves.load(Ordering::SeqCst), 1);
    Ok(())
}
#[tokio::test]
async fn wss_rejects_untrusted_tls_bad_hostname_and_invalid_upgrades() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    for reply in [
        Reply::InvalidAccept,
        Reply::Redirect,
        Reply::Extensions,
        Reply::Large,
    ] {
        let p = peer(reply, vec![], true).await?;
        let t = transport(&a, &p)?;
        let l = Ledger::new();
        assert_eq!(
            t.connect(&s, &l, &a, &cancel)
                .await
                .err()
                .ok_or("expected rejection")?
                .kind(),
            WssErrorKind::Protocol
        );
        assert_eq!(l.reserves.load(Ordering::SeqCst), 1);
    }
    let p = peer(Reply::Valid, vec![], false).await?;
    let t = transport(&a, &p)?;
    assert_eq!(
        t.connect(&s, &Ledger::new(), &a, &cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Protocol
    );
    let other = peer(Reply::Valid, vec![], true).await?;
    let t = WssTransport::build(config(&a, &other.url)?, p.roots.clone())?;
    assert_eq!(
        t.connect(&s, &Ledger::new(), &a, &cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Protocol
    );
    Ok(())
}
#[tokio::test]
async fn wss_protocol_overflow_and_lost_charge_are_terminal() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    for wire in [
        vec![0x82, 127, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        vec![0x80, 0],
        vec![0x82, 0x80, 0, 0, 0, 0],
        vec![0x88, 1, 0],
    ] {
        let p = peer(Reply::Valid, wire, true).await?;
        let t = transport(&a, &p)?;
        let l = Ledger::new();
        let mut ws = t.connect(&s, &l, &a, &cancel).await?;
        assert!(ws.receive(&cancel).await.is_err());
        assert_eq!(
            ws.receive(&cancel)
                .await
                .err()
                .ok_or("expected rejection")?
                .kind(),
            WssErrorKind::Stopped
        );
    }
    let p = peer(Reply::Valid, vec![0x82, 1, b'x'], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    l.lose.store(true, Ordering::SeqCst);
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Accounting
    );
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 1);
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Stopped
    );
    let mut wire = Vec::new();
    for size in [9_u8, 9, 5] {
        wire.extend([0x82, size]);
        wire.extend(vec![b'x'; usize::from(size)]);
    }
    let p = peer(Reply::Valid, wire, true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    ws.receive(&cancel).await?;
    ws.receive(&cancel).await?;
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected overage")?
            .kind(),
        WssErrorKind::Budget
    );
    let usage = *l.usage.lock().map_err(|_| "lock")?;
    assert_eq!(usage.bytes, 23);
    assert!(usage.stopped);
    Ok(())
}
#[tokio::test]
async fn wss_cancellation_denial_and_abandoned_receive_never_reuse_socket() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    let p = peer(Reply::Valid, vec![], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    let mut future = Box::pin(ws.receive(&cancel));
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(future);
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Stopped
    );
    let l = Ledger::new();
    cancel.cancel();
    assert_eq!(
        t.connect(&s, &l, &a, &cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Cancelled
    );
    assert_eq!(l.reserves.load(Ordering::SeqCst), 0);
    let cancel = Cancellation::default();
    let p = peer(Reply::Valid, vec![0x82, 0], true).await?;
    let t = transport(&a, &p)?;
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    a.revoked.store(true, Ordering::SeqCst);
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Authority
    );
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 0);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn wss_guard_bounds_idle_partial_work_and_total_lifetime() -> TestResult {
    // Timer proof uses an injected permanently pending operation, not timing-sensitive socket sleeps.
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    let mut last = AT;
    let until = Instant::now() + Duration::from_millis(500);
    let error = guard(
        &a,
        &s,
        &mut last,
        until,
        Duration::from_millis(10),
        &cancel,
        std::future::pending::<Result<()>>(),
    )
    .await
    .err()
    .ok_or("expected rejection")?;
    assert_eq!(error.kind(), WssErrorKind::Timeout);
    let mut last = AT;
    let cancellation = Cancellation::default();
    let trigger = cancellation.clone();
    let task = tokio::spawn(async move {
        tokio::task::yield_now().await;
        trigger.cancel();
    });
    assert_eq!(
        guard(
            &a,
            &s,
            &mut last,
            Instant::now() + Duration::from_secs(5),
            Duration::from_millis(10),
            &cancellation,
            std::future::pending::<Result<()>>()
        )
        .await
        .err()
        .ok_or("expected rejection")?
        .kind(),
        WssErrorKind::Cancelled
    );
    task.await?;
    Ok(())
}
#[test]
fn wss_configuration_and_diagnostics_fail_closed() -> TestResult {
    let a = Authority::new()?;
    for endpoint in [
        "ws://127.0.0.1/",
        "wss://u:p@example.invalid/",
        "wss://example.invalid/?token=x",
        "wss://example.invalid/#x",
    ] {
        assert!(WssTransport::new(config(&a, endpoint)?).is_err());
    }
    let frame = WssFrame {
        ordinal: 1,
        received_at: at(AT + 1)?,
        kind: WssFrameKind::Text,
        final_fragment: true,
        payload: b"licensed-sentinel".to_vec(),
    };
    let error = WssError::caused(
        WssErrorKind::Protocol,
        std::io::Error::other("licensed-sentinel"),
    );
    let diagnostics = format!("{frame:?} {error:?} {error}");
    assert!(!diagnostics.contains("licensed-sentinel"));
    assert!(!diagnostics.contains(&format!("{:?}", frame.payload())));
    assert!(std::error::Error::source(&error).is_some());
    Ok(())
}

#[tokio::test]
async fn wss_abandoned_committed_charge_never_exposes_or_replays_payload() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    let p = peer(Reply::Valid, vec![0x82, 1, b'x'], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    l.stall.store(true, Ordering::SeqCst);
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    let mut future = Box::pin(ws.receive(&cancel));
    tokio::select! {
        _=l.charged.notified()=>{},
        _=&mut future=>return Err("receive completed without accounting confirmation".into()),
    }
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 1);
    drop(future);
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Stopped
    );
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 1);
    Ok(())
}

#[tokio::test]
async fn wss_fragment_and_outgoing_budgets_are_not_renewed_by_progress() -> TestResult {
    let a = Authority::new()?;
    let mut data = session_data(&a)?;
    data.limits.bytes = std::num::NonZeroU64::new(100).ok_or("fixture")?;
    let s = a.fixture.seal(data)?;
    let cancel = Cancellation::default();
    let mut wire = Vec::new();
    for header in [0x02, 0x00, 0x80] {
        wire.extend([header, 8]);
        wire.extend([b'x'; 8]);
    }
    let p = peer(Reply::Valid, wire, true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    ws.receive(&cancel).await?;
    ws.receive(&cancel).await?;
    // Each fragment is charged, including the one exceeding the aggregate-message ceiling.
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Budget
    );
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 3);
    let p = peer(Reply::Valid, vec![], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    ws.send(WssFrameKind::Text, b"first", &cancel).await?;
    ws.send(WssFrameKind::Binary, b"second", &cancel).await?;
    assert_eq!(
        ws.send(WssFrameKind::Text, b"third", &cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Budget
    );
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Stopped
    );
    let p = peer(Reply::Valid, vec![], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    // Byte bounds win before UTF-8 validation: this invalid text must never be scanned.
    assert_eq!(
        ws.send(WssFrameKind::Text, &[0xff; 21], &cancel)
            .await
            .err()
            .ok_or("expected byte bound")?
            .kind(),
        WssErrorKind::Budget
    );
    Ok(())
}
#[tokio::test]
async fn wss_frame_ceiling_is_inclusive_and_silent_upgrade_times_out() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a)?;
    let cancel = Cancellation::default();
    for size in [10_u8, 11] {
        let mut wire = vec![0x82, size];
        wire.extend(vec![b'x'; usize::from(size)]);
        let p = peer(Reply::Valid, wire, true).await?;
        let t = transport(&a, &p)?;
        let l = Ledger::new();
        let mut ws = t.connect(&s, &l, &a, &cancel).await?;
        if size == 10 {
            assert_eq!(ws.receive(&cancel).await?.payload().len(), 10);
        } else {
            assert!(ws.receive(&cancel).await.is_err());
            assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 0);
        }
    }
    let p = peer(Reply::Silent, vec![], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut connecting = Box::pin(t.connect(&s, &l, &a, &cancel));
    tokio::select! {
        _ = p.request_read.notified() => {},
        _ = &mut connecting => return Err("connect finished before silent-peer barrier".into()),
    }
    // TLS and the request write really completed; only the upgrade reply is withheld.
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(5001)).await;
    assert_eq!(
        connecting.await.err().ok_or("expected timeout")?.kind(),
        WssErrorKind::Timeout
    );
    tokio::time::resume();
    assert_eq!(l.reserves.load(Ordering::SeqCst), 1);
    let p = peer(Reply::Valid, vec![], true).await?;
    let t = transport(&a, &p)?;
    let l = Ledger::new();
    let mut ws = t.connect(&s, &l, &a, &cancel).await?;
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(5001)).await;
    assert_eq!(
        ws.receive(&cancel)
            .await
            .err()
            .ok_or("expected rejection")?
            .kind(),
        WssErrorKind::Timeout
    );
    assert_eq!(l.usage.lock().map_err(|_| "lock")?.frames, 0);
    Ok(())
}
