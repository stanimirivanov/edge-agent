//! Invented protocol fixtures only; no provider data, credentials or remote requests.
use super::metered::support::{Ledger, authority};
use super::support::*;
use crate::*;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::sync::atomic::Ordering;

fn schedule(f: &Fixture) -> TestResult<DateOnlySchedule> {
    let scope = RoutingScope {
        source: f.request.scope.source,
        feed: FeedId::new(1)?,
        epoch: FeedEpoch::new(1)?,
    };
    let effective = EffectiveInterval::new(at(0)?, at(NOW + 86400000)?)?;
    let mapping = ReferenceVersion::new(
        ReferenceId::new(4)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [4; 32],
        InstrumentMapping::new(
            f.request.scope.instrument,
            ListingId::new(1)?,
            Symbol::new("INVENTED")?,
            Venue::Xnys,
            f.request.range,
        ),
    );
    let directory = RoutingDirectory::new(
        scope,
        effective,
        vec![RouteBinding::new(
            RoutingKey::symbol(&Symbol::new("INVENTED")?),
            mapping,
        )?],
    )?;
    let directories = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(5)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [5; 32],
        directory,
    )])?;
    let selections = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(6)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [6; 32],
        InstrumentSelection::new(effective, &[f.request.scope.instrument])?,
    )])?;
    let plan = SelectionPlan::resolve(
        SelectionContext {
            scope,
            effective_at: f.request.admitted_at,
            cutoff: f.request.admitted_at,
            session: f.request.range.bounds().0,
        },
        (ReferenceId::new(6)?, ReferenceId::new(5)?),
        &selections,
        &directories,
    )?;
    let selected = plan.prepare(&f.request)?;
    let date = f.request.range.bounds().0;
    let calendar = ReferenceVersion::new(
        ReferenceId::new(2)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [2; 32],
        CalendarDay::new(
            Venue::Xnys,
            date,
            SessionHours::Regular(SessionWindow::new(at(3600000)?, at(7200000)?)?),
            "synthetic-2026a",
        )?,
    );
    let bucket = ReferenceVersion::new(
        ReferenceId::new(3)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [3; 32],
        DateOnlyBucket::new(
            date,
            EvidenceInterval::new(
                PreciseInstant::from_millis(at(0)?),
                PreciseInstant::from_millis(at(86400000)?),
            )?,
            SemanticReference::new(calendar.id(), calendar.revision()),
        )?,
    );
    assert!(DateOnlySchedule::new(&selected, &[], std::slice::from_ref(&bucket)).is_err());
    assert!(DateOnlySchedule::new(&selected, std::slice::from_ref(&calendar), &[]).is_err());
    assert!(
        DateOnlySchedule::new(
            &selected,
            std::slice::from_ref(&calendar),
            &[bucket.clone(), bucket.clone()]
        )
        .is_err()
    );
    let future = ReferenceVersion::new(
        calendar.id(),
        calendar.revision(),
        calendar.source(),
        at(NOW + 1)?,
        *calendar.checksum(),
        calendar.value().clone(),
    );
    assert!(DateOnlySchedule::new(&selected, &[future], std::slice::from_ref(&bucket)).is_err());
    let result = DateOnlySchedule::new(
        &selected,
        std::slice::from_ref(&calendar),
        std::slice::from_ref(&bucket),
    )?;
    let changed = ReferenceVersion::new(
        calendar.id(),
        calendar.revision(),
        calendar.source(),
        calendar.known_at(),
        *calendar.checksum(),
        CalendarDay::new(
            Venue::Xnys,
            date,
            SessionHours::EarlyClose(SessionWindow::new(at(3600000)?, at(7000000)?)?),
            "synthetic-2026a",
        )?,
    );
    let changed = DateOnlySchedule::new(&selected, &[changed], &[bucket])?;
    assert_ne!(
        result.configuration_digest(),
        changed.configuration_digest(),
        "reused checksums cannot hide changed calendar values"
    );
    Ok(result)
}
fn body() -> &'static [u8] {
    br#"{"status":"ok","meta":{"symbol":"INVENTED","interval":"1day","currency":"USD","exchange_timezone":"America/New_York","exchange":"Invented Exchange","mic_code":"XNYS","type":"Common Stock"},"values":[{"datetime":"1970-01-01","open":"9007199254740993.12345678","high":"9007199254740993.12345679","low":"9007199254740993.12345677","close":"9007199254740993.12345678","volume":"0"}]}"#
}
fn ledger(f: &Fixture, schedule: &DateOnlySchedule) -> TestResult<Ledger> {
    let mut ledger = Ledger::new(f)?;
    ledger.record = ledger
        .record
        .with_configuration(schedule.configuration_digest())?;
    Ok(ledger)
}
fn config(schedule: DateOnlySchedule) -> TestResult<TwelveDataConfig<'static>> {
    Ok(TwelveDataConfig {
        schedule,
        credential: "invented-local-token",
        cancellation_poll_ms: nz(5)?,
    })
}
#[tokio::test]
async fn twelve_data_selected_request_is_raw_header_only_and_exact_native_capture_round_trips()
-> TestResult {
    let f = Fixture::native(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", body()), false).await?;
    let client = TwelveDataDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut session = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    let artifact = session.fetch(&cancel).await?;
    assert_eq!(
        artifact.data().rows[0].ohlc[0].coefficient,
        "900719925474099312345678"
    );
    assert_eq!(artifact.data().rows[0].ohlc[0].scale, 8);
    assert_eq!(artifact.data().rows[0].volume.coefficient, "0");
    assert!(artifact.data().rows[0].time.received_at.seconds >= NOW / 1000);
    assert_eq!(
        artifact.data().response_sha256,
        DailyEvidenceArtifact::hash(body())
    );
    assert_eq!(
        DateOnlyDailyArtifact::decode(artifact.bytes(), artifact.digest())?,
        artifact
    );
    assert!(matches!(
        DailyObject::decode(artifact.bytes(), artifact.digest())?,
        DailyObject::DateOnly(_)
    ));
    assert!(DailyEvidenceArtifact::decode(artifact.bytes(), artifact.digest()).is_err());
    assert!(!String::from_utf8_lossy(artifact.bytes()).contains("event_at"));
    assert_eq!(
        AdmissionRecord::decode(ledger.record.bytes(), ledger.record.command())?,
        ledger.record
    );
    ledger.record.verify_authority(&f.admit(&cancel)?)?;
    assert_eq!(session.usage().await?.durable.calls, 1);
    assert!(session.fetch(&cancel).await.is_err());
    assert_eq!(server.count()?, 1);
    let requests = server.requests.lock().map_err(|_| "fixture lock")?;
    let request = String::from_utf8_lossy(requests.first().ok_or("request")?);
    let line = request.lines().next().ok_or("request line")?;
    let target = line.split_whitespace().nth(1).ok_or("request target")?;
    let url = url::Url::parse(&format!("http://loopback.invalid{target}"))?;
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "end_date" && value == "1970-01-01T23:59:59")
    );
    for parameter in [
        "symbol=INVENTED",
        "mic_code=XNYS",
        "interval=1day",
        "adjust=none",
        "order=asc",
        "start_date=1970-01-01",
        "end_date=1970-01-01",
        "outputsize=366",
    ] {
        assert!(line.contains(parameter), "missing {parameter}");
    }
    assert!(!line.contains("token"));
    assert!(!line.contains("apikey"));
    assert!(!line.contains("timezone"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: apikey invented-local-token")
    );
    assert!(!format!("{client:?} {session:?} {artifact:?}").contains("invented-local-token"));
    Ok(())
}
#[tokio::test]
async fn twelve_data_foreign_null_duplicate_incomplete_and_imprecise_responses_fail_closed()
-> TestResult {
    let original = std::str::from_utf8(body())?;
    let row = original
        .split("\"values\":[")
        .nth(1)
        .ok_or("row")?
        .strip_suffix("]}")
        .ok_or("tail")?;
    let mut mutations = vec![
        original.replace("INVENTED", "FOREIGN"),
        original.replace("XNYS", "XNAS"),
        original.replace("USD", "EUR"),
        original.replace("America/New_York", "UTC"),
        original.replace("Common Stock", "ETF"),
        original.replace(
            "\"currency\":\"USD\"",
            "\"currency\":\"USD\",\"adjust\":\"splits\"",
        ),
        original.replace(
            "\"currency\":\"USD\"",
            "\"currency\":\"USD\",\"adjusted\":true",
        ),
        original.replace("1day", "1min"),
        original.replace("1970-01-01", "1970-01-02"),
        original.replace("1970-01-01", "1970-01-01 00:00:00"),
        original.replace("\"volume\":\"0\"", "\"volume\":null"),
        original.replace("\"volume\":\"0\"", "\"volume\":\"-1\""),
        original.replace(".12345678", ".123456789"),
        original.replace("\"status\":\"ok\"", "\"status\":\"ok\",\"status\":\"ok\""),
        original.replace(row, ""),
        original.replace(row, &format!("{row},{row}")),
    ];
    mutations.push(original.replace("\"high\":\"9007199254740993.12345679\"", "\"high\":\"1\""));
    for body in mutations {
        let f = Fixture::native(42)?;
        let schedule = schedule(&f)?;
        let ledger = ledger(&f, &schedule)?;
        let authority = authority(&f);
        let server = Server::start(response(200, "", body.as_bytes()), false).await?;
        let client = TwelveDataDaily::for_loopback(config(schedule)?, &server.url)?;
        let mut s = client
            .start(
                &ledger,
                &ledger.record,
                &authority,
                &Cancellation::default(),
            )
            .await?;
        assert!(s.fetch(&Cancellation::default()).await.is_err());
        assert_eq!(s.usage().await?.durable.calls, 1);
        assert_eq!(server.count()?, 1);
    }
    Ok(())
}
#[tokio::test]
async fn twelve_data_errors_cancellation_and_authority_denial_never_retry_or_bypass_quota()
-> TestResult {
    for (code, expected) in [
        (401, ProviderFailure::Forbidden),
        (403, ProviderFailure::Forbidden),
        (
            429,
            ProviderFailure::Transient {
                retry_after_ms: None,
            },
        ),
        (
            500,
            ProviderFailure::Transient {
                retry_after_ms: None,
            },
        ),
        (400, ProviderFailure::InvalidResponse),
    ] {
        let f = Fixture::native(42)?;
        let schedule = schedule(&f)?;
        let ledger = ledger(&f, &schedule)?;
        let authority = authority(&f);
        let body =
            format!("{{\"status\":\"error\",\"code\":{code},\"message\":\"PRIVATE-SENTINEL\"}}");
        let server = Server::start(response(200, "", body.as_bytes()), false).await?;
        let client = TwelveDataDaily::for_loopback(config(schedule)?, &server.url)?;
        let cancel = Cancellation::default();
        let mut s = client
            .start(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        let error = s
            .fetch(&cancel)
            .await
            .err()
            .ok_or("provider error accepted")?;
        assert!(
            matches!(&error,DispatchError::Http(e) if e.kind()==HttpErrorKind::Provider(expected))
        );
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        assert_eq!(s.usage().await?.durable.calls, 1);
        assert!(s.fetch(&cancel).await.is_err());
        assert_eq!(server.count()?, 1);
    }
    let f = Fixture::native(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", body()), false).await?;
    let client = TwelveDataDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    cancel.cancel();
    assert!(s.fetch(&cancel).await.is_err());
    assert_eq!(s.usage().await?.durable.calls, 0);
    assert_eq!(server.count()?, 0);
    authority.deny.store(true, Ordering::SeqCst);
    assert!(
        client
            .start(
                &ledger,
                &ledger.record,
                &authority,
                &Cancellation::default()
            )
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn twelve_data_lost_confirmation_releases_original_receipt_without_refetch() -> TestResult {
    let mut f = Fixture::native(42)?;
    f.request.deadline = at(NOW + 60000)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", body()), false).await?;
    let client = TwelveDataDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    ledger.lose_complete.store(true, Ordering::SeqCst);
    assert!(
        matches!(s.fetch(&cancel).await,Err(DispatchError::Budget(e)) if e.kind()==BudgetErrorKind::Storage)
    );
    assert!(s.usage().await?.awaiting_confirmation);
    assert!(s.fetch(&cancel).await.is_err());
    ledger.set_time(&authority, NOW + 15000);
    let artifact = s.confirm(&cancel).await?;
    assert!(artifact.data().rows[0].time.received_at.seconds < (NOW + 15000) / 1000);
    assert_eq!(
        artifact.data().rows[0].time.derived_at.seconds,
        (NOW + 15000) / 1000
    );
    assert_eq!(s.usage().await?.durable.calls, 1);
    assert_eq!(server.count()?, 1);
    assert!(s.confirm(&cancel).await.is_err());
    Ok(())
}
#[tokio::test]
async fn twelve_data_configuration_and_native_format_cannot_weaken_authority() -> TestResult {
    let f = Fixture::native(42)?;
    let schedule = schedule(&f)?;
    let ledger = Ledger::new(&f)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", body()), false).await?;
    let client = TwelveDataDaily::for_loopback(config(schedule.clone())?, &server.url)?;
    assert!(
        client
            .start(
                &ledger,
                &ledger.record,
                &authority,
                &Cancellation::default()
            )
            .await
            .is_err()
    );
    let mut missing = config(schedule.clone())?;
    missing.credential = "";
    assert!(TwelveDataDaily::for_loopback(missing, &server.url).is_err());
    assert!(
        TwelveDataDaily::for_loopback(config(schedule.clone())?, "https://external.invalid/data")
            .is_err()
    );
    let non_native = Fixture::new(42)?;
    assert!(super::twelve_data::schedule(&non_native).is_err());
    assert_eq!(server.count()?, 0);
    let ledger = super::twelve_data::ledger(&f, &schedule)?;
    let mut exhausted = Ledger::with_opening_debit(&f, 100)?;
    exhausted.record = exhausted
        .record
        .with_configuration(schedule.configuration_digest())?;
    let mut blocked = client
        .start(
            &exhausted,
            &exhausted.record,
            &authority,
            &Cancellation::default(),
        )
        .await?;
    assert!(blocked.fetch(&Cancellation::default()).await.is_err());
    assert_eq!(blocked.usage().await?.durable.calls, 0);
    assert_eq!(
        server.count()?,
        0,
        "spent daily allocation must deny before HTTP"
    );
    let mut s = client
        .start(
            &ledger,
            &ledger.record,
            &authority,
            &Cancellation::default(),
        )
        .await?;
    let artifact = s.fetch(&Cancellation::default()).await?;
    let mut data = artifact.data().clone();
    data.rows.push(data.rows[0].clone());
    assert!(DateOnlyDailyArtifact::new(data).is_err());
    let mut data = artifact.data().clone();
    data.rows[0].volume.scale = 7;
    assert!(DateOnlyDailyArtifact::new(data).is_err());
    let mut data = artifact.data().clone();
    data.rows[0].time.rule.calendar.revision = EvidenceRevision::new("2")?;
    assert!(DateOnlyDailyArtifact::new(data).is_err());
    for bytes in [
        artifact.bytes().strip_suffix(b"\n").ok_or("LF")?.to_vec(),
        String::from_utf8_lossy(artifact.bytes())
            .replace("date-only-daily-capture.v1", "date-only-daily-capture.v2")
            .into_bytes(),
        vec![b' '; MAX_EVIDENCE_BYTES + 1],
    ] {
        assert!(DailyObject::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes)).is_err());
    }
    let changed = String::from_utf8_lossy(ledger.record.bytes())
        .replace(
            "edgeagent.admission.date-only.v1",
            "edgeagent.admission.daily.v2",
        )
        .into_bytes();
    let mut command = ledger.record.command().clone();
    command.request_sha256 = DailyEvidenceArtifact::hash(&changed);
    assert!(AdmissionRecord::decode(&changed, &command).is_err());
    Ok(())
}
