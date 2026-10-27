//! Invented protocol data and ephemeral loopback TLS only; never Alpaca's test service.
use crate::streaming_fixture as fixture;
mod peer;
use super::*;
use edgeagent_market_data_application::*;
use fixture::{Fixture, at, nz};
use std::{
    num::NonZeroU64,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const AT: i64 = 1_800_000_000_000;
const WELCOME: &[u8] = br#"[{"T":"success","msg":"connected"}]"#;
const AUTH: &[u8] = br#"[{"T":"success","msg":"authenticated"}]"#;
const ACK:&[u8]=br#"[{"T":"subscription","trades":["SYNTH1"],"bars":[],"corrections":["SYNTH1"],"cancelErrors":["SYNTH1"],"quotes":[]}]"#;
const TRADE:&[u8]=br#"[{"T":"t","S":"SYNTH1","i":9007199254740993,"x":"V","p":12.12345678,"s":3,"c":["@"],"t":"2027-01-15T07:59:59.000000001Z","z":"C"}]"#;
fn fixture(bars: bool) -> TestResult<Fixture> {
    let contract = EvidenceContract::new(
        if bars {
            EvidenceKind::RawBar(BarCoverage::Intraday)
        } else {
            EvidenceKind::ShareTrade
        },
        MarketScope::Venue(Venue::Xiex),
        SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
        TimestampPrecision::Nanoseconds,
    )?;
    Fixture::profile(AT, 1, contract, DeliveryCapability::Snapshots)
}
fn session(f: &Fixture) -> TestResult<StreamingSession> {
    let mut data = f.data(1)?;
    data.delivery = DeliveryCapability::Snapshots;
    data.limits.frames = nz(100)?;
    data.limits.frame_bytes = nz(8192)?;
    data.limits.bytes = NonZeroU64::new(65536).ok_or("zero")?;
    data.limits.connect_ms = nz(5000)?;
    data.limits.idle_ms = nz(5000)?;
    f.seal(data)
}
fn advance(d: &mut Decoder<'_>) -> TestResult {
    for bytes in [WELCOME, AUTH, ACK] {
        d.decode(bytes, [1, 1], at(AT + 1)?)?;
    }
    Ok(())
}
#[test]
fn iex_protocol_requires_exact_handshake_and_subscription() -> TestResult {
    let f = fixture(false)?;
    let s = session(&f)?;
    let p = f.plan()?;
    let mut d = Decoder::new(&s, &p, 10)?;
    assert!(d.decode(AUTH, [1, 1], at(AT + 1)?).is_err());
    assert_eq!(
        d.decode(WELCOME, [2, 2], at(AT + 1)?)
            .err()
            .ok_or("expected sticky error")?
            .kind(),
        IexErrorKind::Stopped
    );
    let mut d = Decoder::new(&s, &p, 10)?;
    d.decode(WELCOME, [1, 1], at(AT + 1)?)?;
    d.decode(AUTH, [2, 2], at(AT + 1)?)?;
    let command: serde_json::Value = serde_json::from_slice(&d.subscription()?)?;
    assert_eq!(
        command,
        serde_json::json!({"action":"subscribe","trades":["SYNTH1"]})
    );
    assert!(matches!(
        d.decode(ACK, [3, 3], at(AT + 1)?)?,
        IexProgress::Subscribed
    ));
    for ack in [
        br#"[{"T":"subscription","trades":["*"],"bars":[],"corrections":["SYNTH1"],"cancelErrors":["SYNTH1"]}]"#.as_slice(),
        br#"[{"T":"subscription","trades":["SYNTH1"],"bars":[]}]"#,
        br#"[{"T":"subscription","trades":["SYNTH1","SYNTH1"],"bars":[],"corrections":["SYNTH1"],"cancelErrors":["SYNTH1"]}]"#,
        br#"[{"T":"subscription","trades":["SYNTH1"],"bars":[],"quotes":["SYNTH1"],"corrections":["SYNTH1"],"cancelErrors":["SYNTH1"]}]"#,
        br#"[{"T":"subscription","trades":[],"bars":[]}]"#,
    ] {
        let mut d=Decoder::new(&s,&p,10)?;d.decode(WELCOME,[1,1],at(AT+1)?)?;d.decode(AUTH,[2,2],at(AT+1)?)?;
        assert!(d.decode(ack,[3,3],at(AT+1)?).is_err());
    }
    Ok(())
}
#[test]
fn iex_exact_trades_and_minute_bars_preserve_scope_time_and_values() -> TestResult {
    let f = fixture(false)?;
    let s = session(&f)?;
    let p = f.plan()?;
    let mut d = Decoder::new(&s, &p, 10)?;
    advance(&mut d)?;
    let IexProgress::Records(records) = d.decode(TRADE, [4, 6], at(AT + 1)?)? else {
        return Err("records".into());
    };
    let r = records.first().ok_or("record")?;
    assert_eq!(
        r.mapping.value().identities().0,
        f.scopes.first().ok_or("scope")?.instrument
    );
    assert_eq!(r.contract.shape().1, MarketScope::Venue(Venue::Xiex));
    assert_eq!(r.frames, [4, 6]);
    assert_eq!(r.item, 0);
    assert_eq!(r.received_at, at(AT + 1)?);
    assert_eq!(r.event_at.unix_parts().1, 1);
    let IexValue::Trade { id, value, .. } = &r.value else {
        return Err("trade".into());
    };
    assert_eq!(*id, 9_007_199_254_740_993);
    assert_eq!(value.values().0.decimal_parts(), (1_212_345_678, 8));
    let f = fixture(true)?;
    let s = session(&f)?;
    let p = f.plan()?;
    let mut d = Decoder::new(&s, &p, 10)?;
    d.decode(WELCOME, [1, 1], at(AT + 1)?)?;
    d.decode(AUTH, [2, 2], at(AT + 1)?)?;
    d.decode(
        br#"[{"T":"subscription","trades":[],"bars":["SYNTH1"]}]"#,
        [3, 3],
        at(AT + 1)?,
    )?;
    let IexProgress::Records(records)=d.decode(br#"[{"T":"b","S":"SYNTH1","o":12,"h":13,"l":11,"c":12.5,"v":3.25,"n":2,"vw":12.25,"t":"2027-01-15T07:59:00Z"}]"#,[4,4],at(AT+1)?)? else{return Err("bar".into())};
    let IexValue::MinuteBar {
        value,
        trades,
        vwap,
    } = &records.first().ok_or("bar")?.value
    else {
        return Err("bar".into());
    };
    assert_eq!(*trades, 2);
    assert_eq!(vwap.decimal_parts(), (1225, 2));
    assert_eq!(value.values().1.decimal_parts(), (325, 2));
    let (start, end) = value.bucket().0.bounds();
    assert_eq!(end.unix_parts().0 - start.unix_parts().0, 60);
    let bar = r#"[{"T":"b","S":"SYNTH1","o":12,"h":13,"l":11,"c":12.5,"v":3.25,"n":2,"vw":12.25,"t":"2027-01-15T07:59:00Z"}]"#;
    for malformed in [
        bar.replace("07:59:00Z", "07:59:01Z"),
        bar.replace("07:59:00Z", "08:00:00Z"),
        bar.replace("\"h\":13", "\"h\":10"),
        bar.replace("12.5", "12.123456789"),
        bar.replace("\"n\":2", "\"n\":-1"),
        bar.replace("00Z", "00+00:00"),
    ] {
        let mut d = Decoder::new(&s, &p, 10)?;
        d.decode(WELCOME, [1, 1], at(AT + 1)?)?;
        d.decode(AUTH, [2, 2], at(AT + 1)?)?;
        d.decode(
            br#"[{"T":"subscription","trades":[],"bars":["SYNTH1"]}]"#,
            [3, 3],
            at(AT + 1)?,
        )?;
        assert!(d.decode(malformed.as_bytes(), [4, 4], at(AT + 1)?).is_err());
    }
    Ok(())
}
#[test]
fn iex_batches_reject_ambiguity_corrections_and_partial_success() -> TestResult {
    let f = fixture(false)?;
    let s = session(&f)?;
    let p = f.plan()?;
    for tail in [
        r#"{"T":"error","code":407,"msg":"sensitive"}"#,
        r#"{"T":"c","S":"UNSELECTED"}"#,
        r#"{"T":"x","S":"UNSELECTED"}"#,
        r#"{"T":"u","S":"UNSELECTED"}"#,
        r#"{"T":"success","msg":"authenticated"}"#,
        r#"{"T":"q","S":"SYNTH1"}"#,
    ] {
        let mut d = Decoder::new(&s, &p, 10)?;
        advance(&mut d)?;
        let first = std::str::from_utf8(TRADE)?.trim_end_matches(']');
        let batch = format!("{first},{tail}]");
        assert!(d.decode(batch.as_bytes(), [4, 4], at(AT + 1)?).is_err());
        assert_eq!(
            d.decode(TRADE, [5, 5], at(AT + 1)?)
                .err()
                .ok_or("expected sticky error")?
                .kind(),
            IexErrorKind::Stopped
        );
    }
    for bytes in [
        br#"[{"T":"t","T":"b","S":"SYNTH1"}]"#.as_slice(),
        br#"[{"T":"t","S":"UNKNOWN","p":"bad"}]"#,
        b"[]",
        b"{}",
        b"[\xff]",
    ] {
        let mut d = Decoder::new(&s, &p, 10)?;
        advance(&mut d)?;
        assert!(d.decode(bytes, [4, 4], at(AT + 1)?).is_err());
    }
    let mut d = Decoder::new(&s, &p, 1)?;
    advance(&mut d)?;
    let text = std::str::from_utf8(TRADE)?
        .trim_start_matches('[')
        .trim_end_matches(']');
    assert_eq!(
        d.decode(format!("[{text},{text}]").as_bytes(), [4, 4], at(AT + 1)?)
            .err()
            .ok_or("expected cap error")?
            .kind(),
        IexErrorKind::Budget
    );
    for needle in ["12.12345678", "9007199254740993"] {
        let malformed = std::str::from_utf8(TRADE)?.replace(needle, &format!("\"{needle}\""));
        let mut d = Decoder::new(&s, &p, 10)?;
        advance(&mut d)?;
        assert!(d.decode(malformed.as_bytes(), [4, 4], at(AT + 1)?).is_err());
    }
    Ok(())
}
#[test]
fn iex_known_unselected_payload_skips_values_but_not_control_or_mapping_checks() -> TestResult {
    let template = fixture(false)?;
    let mut f = Fixture::profile(AT, 2, template.contract, DeliveryCapability::Snapshots)?;
    f.retain_first(1)?;
    let s = session(&f)?;
    let p = f.plan()?;
    let mut d = Decoder::new(&s, &p, 10)?;
    advance(&mut d)?;
    let text = std::str::from_utf8(TRADE)?.trim_start_matches('[');
    let batch = format!(r#"[{{"T":"t","S":"SYNTH2","p":"not-a-price"}},{text}"#);
    let IexProgress::Records(records) = d.decode(batch.as_bytes(), [4, 4], at(AT + 1)?)? else {
        return Err("records".into());
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records.first().ok_or("record")?.item, 1);
    assert_eq!(
        d.decode(br#"[{"T":"x","S":"SYNTH2"}]"#, [5, 5], at(AT + 1)?)
            .err()
            .ok_or("expected control error")?
            .kind(),
        IexErrorKind::Reconciliation
    );
    // Equal IDs/checksums must not hide different selection content.
    let other = fixture(false)?;
    let other_plan = other.plan()?;
    assert!(Decoder::new(&s, &other_plan, 10).is_err());
    Ok(())
}
struct Authority {
    f: Fixture,
    now: UtcInstant,
    revoked: AtomicBool,
    rights: ReferenceHistory<RightsRecord>,
}
impl Authority {
    fn new() -> TestResult<Self> {
        let f = fixture(false)?;
        let rights = f.revoked()?;
        Ok(Self {
            f,
            now: at(AT + 1)?,
            revoked: AtomicBool::new(false),
            rights,
        })
    }
}
impl StreamingAuthority for Authority {
    fn now(&self) -> UtcInstant {
        self.now
    }
    fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: if self.revoked.load(Ordering::SeqCst) {
                &self.rights
            } else {
                &self.f.rights
            },
            providers: &self.f.providers,
        }
    }
}
struct Ledger {
    usage: Mutex<StreamingUsage>,
    lost: AtomicBool,
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
            lost: AtomicBool::new(false),
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
        StreamingConnection::from_committed(
            s,
            1,
            *self
                .usage
                .lock()
                .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?,
        )
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
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?;
        let frame = s.data().limits.charge(*usage, a.ordinal, a.bytes)?;
        *usage = frame.usage;
        if self.lost.load(Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Storage));
        }
        Ok(frame)
    }
}
fn config<'a>(f: &Fixture, endpoint: &'a str) -> TestResult<AlpacaIexConfig<'a>> {
    Ok(AlpacaIexConfig {
        transport: WssConfig {
            scope: *f.scopes.first().ok_or("scope")?,
            endpoint,
            bearer: None,
            poll_ms: nz(10)?,
            upgrade_bytes: nz(8192)?,
            message_bytes: nz(8192)?,
            writes: nz(10)?,
            write_bytes: nz(8192)?,
        },
        key_id: "invented",
        secret: "invented",
        handshake_ms: nz(5000)?,
        batch_records: nz(10)?,
    })
}
fn make_adapter(a: &Authority, p: &peer::Peer) -> TestResult<AlpacaIex> {
    let c = config(&a.f, &p.url)?;
    Ok(AlpacaIex {
        transport: WssTransport::for_test(c.transport, p.roots.clone())?,
        key_id: c.key_id.to_owned(),
        secret: c.secret.to_owned(),
        handshake_ms: c.handshake_ms.get(),
        batch_records: c.batch_records.get(),
        message_bytes: 8192,
    })
}
#[tokio::test]
async fn iex_loopback_tls_auth_subscription_fragments_and_controls_are_charged() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a.f)?;
    let plan = a.f.plan()?;
    let ledger = Ledger::new();
    let cancel = Cancellation::default();
    let split = TRADE.len() / 2;
    let p = peer::start(
        vec![
            (false, 1, TRADE.get(..split).ok_or("slice")?.to_vec()),
            (true, 9, b"ping".to_vec()),
            (true, 0, TRADE.get(split..).ok_or("slice")?.to_vec()),
        ],
        false,
    )
    .await?;
    let adapter = make_adapter(&a, &p)?;
    let mut stream = adapter.connect(&s, &plan, &ledger, &a, &cancel).await?;
    for _ in 0..2 {
        assert!(matches!(stream.next(&cancel).await?, IexProgress::Control))
    }
    assert!(matches!(
        stream.next(&cancel).await?,
        IexProgress::Subscribed
    ));
    assert!(matches!(stream.next(&cancel).await?, IexProgress::Control));
    let IexProgress::Records(records) = stream.next(&cancel).await? else {
        return Err("records".into());
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records.first().ok_or("record")?.frames, [4, 6]);
    assert_eq!(ledger.usage.lock().map_err(|_| "lock")?.frames, 6);
    assert_eq!(p.commands.lock().map_err(|_| "lock")?.len(), 2);
    Ok(())
}
#[tokio::test]
async fn iex_unknown_accounting_revocation_and_cancellation_stop_without_reconnect() -> TestResult {
    for fault in 0..3 {
        let a = Authority::new()?;
        let s = session(&a.f)?;
        let plan = a.f.plan()?;
        let ledger = Ledger::new();
        let cancel = Cancellation::default();
        let p = peer::start(vec![(true, 1, TRADE.to_vec())], false).await?;
        let adapter = make_adapter(&a, &p)?;
        let mut stream = adapter.connect(&s, &plan, &ledger, &a, &cancel).await?;
        if fault == 0 {
            ledger.lost.store(true, Ordering::SeqCst)
        } else if fault == 1 {
            a.revoked.store(true, Ordering::SeqCst)
        } else {
            cancel.cancel()
        }
        let error = stream.next(&cancel).await.err().ok_or("expected denial")?;
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(
            error.kind(),
            IexErrorKind::Transport(match fault {
                0 => WssErrorKind::Accounting,
                1 => WssErrorKind::Authority,
                _ => WssErrorKind::Cancelled,
            })
        );
        assert_eq!(
            stream
                .next(&cancel)
                .await
                .err()
                .ok_or("expected stopped")?
                .kind(),
            IexErrorKind::Stopped
        );
        assert_eq!(
            ledger.usage.lock().map_err(|_| "lock")?.frames,
            u64::from(fault == 0)
        );
    }
    Ok(())
}
#[tokio::test]
async fn iex_abandoned_intake_and_absolute_handshake_deadline_are_terminal() -> TestResult {
    let a = Authority::new()?;
    let s = session(&a.f)?;
    let plan = a.f.plan()?;
    let ledger = Ledger::new();
    let cancel = Cancellation::default();
    let p = peer::start(vec![], true).await?;
    let adapter = make_adapter(&a, &p)?;
    let mut stream = adapter.connect(&s, &plan, &ledger, &a, &cancel).await?;
    // Poll exactly once: abandonment is independent of wall-clock scheduling.
    {
        let future = stream.next(&cancel);
        tokio::pin!(future);
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }
    assert_eq!(
        stream
            .next(&cancel)
            .await
            .err()
            .ok_or("expected abandoned")?
            .kind(),
        IexErrorKind::Stopped
    );
    assert!(stream.connection().is_none());
    drop(stream);
    let p = peer::start(vec![], true).await?;
    let adapter = make_adapter(&a, &p)?;
    let mut stream = adapter.connect(&s, &plan, &ledger, &a, &cancel).await?;
    stream.handshake_deadline = tokio::time::Instant::now();
    assert_eq!(
        stream
            .next(&cancel)
            .await
            .err()
            .ok_or("expected deadline")?
            .kind(),
        IexErrorKind::Budget
    );
    assert_eq!(
        stream
            .next(&cancel)
            .await
            .err()
            .ok_or("expected stopped")?
            .kind(),
        IexErrorKind::Stopped
    );
    Ok(())
}
#[test]
fn iex_configuration_and_diagnostics_do_not_expand_scope_or_expose_data() -> TestResult {
    let f = fixture(false)?;
    for endpoint in [
        "wss://stream.data.alpaca.markets/v2/sip",
        "wss://stream.data.alpaca.markets/v2/iex?key=invented",
        "ws://localhost/v2/iex",
    ] {
        assert_eq!(
            AlpacaIex::validate(&config(&f, endpoint)?)
                .err()
                .ok_or("expected endpoint error")?
                .kind(),
            IexErrorKind::Configuration
        );
    }
    let c = config(&f, "wss://stream.data.alpaca.markets/v2/iex")?;
    AlpacaIex::validate(&c)?;
    assert!(!format!("{c:?}").contains("invented"));
    let mut c = c;
    c.batch_records = nz(1025)?;
    assert!(AlpacaIex::validate(&c).is_err());
    let wrong = Fixture::new(AT, 1)?;
    let s = wrong.session(1)?;
    let p = wrong.plan()?;
    assert!(Decoder::new(&s, &p, 1).is_err());
    let s = session(&f)?;
    let p = f.plan()?;
    let mut d = Decoder::new(&s, &p, 10)?;
    advance(&mut d)?;
    let result = d.decode(TRADE, [4, 4], at(AT + 1)?)?;
    for private in ["SYNTH1", "12.12345678", "9007199254740993"] {
        assert!(!format!("{result:?}").contains(private))
    }
    Ok(())
}
