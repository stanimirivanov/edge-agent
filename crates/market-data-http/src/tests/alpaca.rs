//! Invented daily SIP protocol only. No account, provider data or external request.
mod support;
use super::metered::support::{Ledger, authority};
use super::support::*;
use crate::*;
use edgeagent_contracts::evidence::DailyEvidenceArtifact;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn alpaca_raw_sip_binding_preserves_exact_provider_instants_and_receipts() -> TestResult {
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 0)?;
    let authority = authority(&f);
    let bytes = page(&[row(0), row(1)], None);
    let server = Server::start(response(200, "", &bytes), false).await?;
    let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    let AlpacaProgress::Complete(capture) = s.fetch_page(&cancel).await? else {
        return Err("incomplete fixture".into());
    };
    assert_eq!(capture.batch().observations.len(), 2);
    let first = &capture.batch().observations[0];
    assert_eq!(first.times().event_at().unix_parts(), (-172800, 7));
    assert_eq!(first.times().provider_times(), (None, None));
    assert_eq!(
        capture.receipts()[0].body_sha256,
        DailyEvidenceArtifact::hash(&bytes)
    );
    assert_eq!(
        first.times().received_at(),
        PreciseInstant::from_millis(capture.receipts()[0].received_at)
    );
    assert_eq!(first.context().origin().2, TransformationRevision::new(7)?);
    let Some(ObservationValue::Bar(bar)) = first.value() else {
        return Err("missing bar".into());
    };
    assert_eq!(
        bar.values().0[0].decimal_parts(),
        (900719925474099312345678, 8)
    );
    assert_eq!(s.usage().await?.durable.calls, 1);
    validate_batch(
        &f.admit(&cancel)?,
        f.catalogs(),
        at(NOW + 1000)?,
        &cancel,
        capture.batch(),
    )?;
    assert!(s.fetch_page(&cancel).await.is_err());
    assert_eq!(server.count()?, 1);
    let requests = server.requests.lock().map_err(|_| "fixture lock")?;
    let request = String::from_utf8_lossy(requests.first().ok_or("request")?);
    let target = request
        .lines()
        .next()
        .ok_or("request line")?
        .split_whitespace()
        .nth(1)
        .ok_or("target")?;
    let url = url::Url::parse(&format!("http://loopback.invalid{target}"))?;
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    for (key, value) in [
        ("symbols", "INVENTED"),
        ("feed", "sip"),
        ("adjustment", "raw"),
        ("asof", "-"),
        ("currency", "USD"),
        ("sort", "asc"),
        ("timeframe", "1Day"),
        ("limit", "252"),
        ("start", "1969-12-30T00:00:00.000000007Z"),
        ("end", "1969-12-31T00:00:00.000000007Z"),
    ] {
        assert_eq!(query.get(key).map(|s| s.as_ref()), Some(value));
    }
    assert!(!target.contains("invented"));
    assert!(!target.contains("secret"));
    let headers = request.to_ascii_lowercase();
    assert!(headers.contains("apca-api-key-id: invented-local-id"));
    assert!(headers.contains("apca-api-secret-key: invented-local-token"));
    assert!(!headers.contains("authorization:"));
    assert!(!format!("{client:?} {s:?} {capture:?}").contains("invented-local-token"));
    Ok(())
}
#[tokio::test]
async fn alpaca_pages_charge_separately_and_confirmation_loss_never_refetches() -> TestResult {
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 0)?;
    let authority = authority(&f);
    let first = page(&[row(0)], Some("opaque?/&+token"));
    let second = page(&[row(1)], None);
    let server = Server::sequence(
        vec![response(200, "", &first), response(200, "", &second)],
        false,
    )
    .await?;
    let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    ledger.lose_complete.store(true, Ordering::SeqCst);
    assert!(
        matches!(s.fetch_page(&cancel).await,Err(DispatchError::Budget(e)) if e.kind()==BudgetErrorKind::Storage)
    );
    assert!(s.fetch_page(&cancel).await.is_err());
    ledger.set_time(&authority, NOW + 15000);
    assert!(matches!(
        s.confirm_page(&cancel).await?,
        AlpacaProgress::More
    ));
    assert_eq!(server.count()?, 1);
    let AlpacaProgress::Complete(capture) = s.fetch_page(&cancel).await? else {
        return Err("incomplete".into());
    };
    assert_eq!(s.usage().await?.durable.calls, 2);
    assert_eq!(server.count()?, 2);
    assert_eq!(capture.receipts().len(), 2);
    assert!(capture.receipts()[0].received_at < at(NOW + 15000)?);
    assert!(capture.receipts()[1].received_at >= at(NOW + 15000)?);
    let requests = server.requests.lock().map_err(|_| "lock")?;
    let second = String::from_utf8_lossy(requests.get(1).ok_or("second request")?);
    let target = second
        .lines()
        .next()
        .ok_or("line")?
        .split_whitespace()
        .nth(1)
        .ok_or("target")?;
    assert!(
        url::Url::parse(&format!("http://loopback.invalid{target}"))?
            .query_pairs()
            .any(|(k, v)| k == "page_token" && v == "opaque?/&+token")
    );
    Ok(())
}
#[tokio::test]
async fn alpaca_manual_rate_rejection_never_sleeps_or_consumes_another_call() -> TestResult {
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 0)?;
    let authority = authority(&f);
    let server = Server::sequence(
        vec![
            response(200, "", &page(&[row(0)], Some("next"))),
            response(200, "", &page(&[row(1)], None)),
        ],
        false,
    )
    .await?;
    let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    assert!(matches!(s.fetch_page(&cancel).await?, AlpacaProgress::More));
    assert!(
        matches!(s.fetch_page(&cancel).await,Err(DispatchError::Budget(e)) if e.kind()==BudgetErrorKind::RateLimited)
    );
    assert_eq!(s.usage().await?.durable.calls, 1);
    assert_eq!(server.count()?, 1);
    ledger.set_time(&authority, NOW + 15000);
    assert!(matches!(
        s.fetch_page(&cancel).await?,
        AlpacaProgress::Complete(_)
    ));
    assert_eq!(server.count()?, 2);
    Ok(())
}
#[tokio::test]
async fn alpaca_malformed_foreign_duplicate_and_truncated_pages_fail_closed() -> TestResult {
    let valid = String::from_utf8(page(&[row(0), row(1)], None))?;
    let mutations = vec![
        valid.replace("INVENTED", "FOREIGN"),
        valid.replace(".000000007Z", ".0000000007Z"),
        valid.replace("1969-12-30T00:00:00.000000007Z", "1969-12-30"),
        valid.replace("\"v\":0", "\"v\":null"),
        valid.replace(".12345678", ".123456789"),
        valid.replace("\"v\":0", "\"v\":0,\"v\":0"),
        String::from_utf8(page(&[row(0)], None))?,
        String::from_utf8(page(&[row(1), row(0)], None))?,
        String::from_utf8(page(&[row(0), row(0)], None))?,
        valid.replace(",\"next_page_token\":null", ""),
        String::from_utf8(page(&[row(0)], Some("")))?,
        String::from_utf8(page(
            &[row(0).replace("\"h\":9007199254740993.12345679", "\"h\":1")],
            Some("next"),
        ))?,
        String::from_utf8(page(&[row(0), row(1)], Some("unexpected")))?,
    ];
    for body in mutations {
        let f = Fixture::precise(42)?;
        let schedule = schedule(&f)?;
        let ledger = ledger(&f, &schedule, 0)?;
        let authority = authority(&f);
        let server = Server::start(response(200, "", body.as_bytes()), false).await?;
        let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
        let cancel = Cancellation::default();
        let mut s = client
            .start(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        assert!(s.fetch_page(&cancel).await.is_err());
        assert_eq!(s.usage().await?.durable.calls, 1);
        assert_eq!(server.count()?, 1);
    }
    // A repeated continuation token is terminal even on an otherwise empty page.
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 0)?;
    let authority = authority(&f);
    let server = Server::sequence(
        vec![
            response(200, "", &page(&[row(0)], Some("cycle"))),
            response(200, "", &page(&[], Some("cycle"))),
        ],
        false,
    )
    .await?;
    let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    s.fetch_page(&cancel).await?;
    ledger.set_time(&authority, NOW + 15000);
    assert!(s.fetch_page(&cancel).await.is_err());
    assert_eq!(server.count()?, 2);
    Ok(())
}
#[tokio::test]
async fn alpaca_credentials_authority_historical_delay_and_spent_credits_deny_before_io()
-> TestResult {
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 100)?;
    let authority = authority(&f);
    let mut recent_fixture = Fixture::precise(42)?;
    recent_fixture.request.range =
        DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 3)?)?;
    let recent = shifted_schedule(&recent_fixture, NOW)?;
    let recent_ledger = support::ledger(&recent_fixture, &recent, 0)?;
    let recent_authority = super::metered::support::authority(&recent_fixture);
    let server = Server::start(response(200, "", &page(&[row(0), row(1)], None)), false).await?;
    let recent_client = AlpacaDaily::for_loopback(config(recent)?, &server.url)?;
    assert!(
        recent_client
            .start(
                &recent_ledger,
                &recent_ledger.record,
                &recent_authority,
                &Cancellation::default()
            )
            .await
            .is_err()
    );
    assert_eq!(
        server.count()?,
        0,
        "recent historical SIP must fail before dispatch"
    );
    let mut missing = config(schedule.clone())?;
    missing.secret = "";
    assert!(AlpacaDaily::for_loopback(missing, &server.url).is_err());
    let mut injected = config(schedule.clone())?;
    injected.key_id = "invalid\r\nHost: external.invalid";
    assert!(AlpacaDaily::for_loopback(injected, &server.url).is_err());
    assert!(
        AlpacaDaily::for_loopback(config(schedule.clone())?, "https://external.invalid/bars")
            .is_err()
    );
    let client = AlpacaDaily::for_loopback(config(schedule.clone())?, &server.url)?;
    let cancel = Cancellation::default();
    let unbound = Ledger::new(&f)?;
    assert!(
        client
            .start(&unbound, &unbound.record, &authority, &cancel)
            .await
            .is_err()
    );
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    assert!(s.fetch_page(&cancel).await.is_err());
    assert_eq!(s.usage().await?.durable.calls, 0);
    assert_eq!(server.count()?, 0);
    assert!(!schedule.satisfies_embargo(at(0)?, 900000));
    authority.deny.store(true, Ordering::SeqCst);
    assert!(
        client
            .start(&ledger, &ledger.record, &authority, &cancel)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn alpaca_errors_cancellation_and_uncertain_start_never_trigger_feed_fallback() -> TestResult
{
    for status in [401, 403, 429, 500] {
        let f = Fixture::precise(42)?;
        let schedule = schedule(&f)?;
        let ledger = ledger(&f, &schedule, 0)?;
        let authority = authority(&f);
        let server = Server::start(response(status, "", b"PRIVATE-SENTINEL"), false).await?;
        let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
        let cancel = Cancellation::default();
        let mut s = client
            .start(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        let error = s.fetch_page(&cancel).await.err().ok_or("status accepted")?;
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        assert!(s.fetch_page(&cancel).await.is_err());
        assert_eq!(server.count()?, 1);
    }
    let f = Fixture::precise(42)?;
    let schedule = schedule(&f)?;
    let ledger = ledger(&f, &schedule, 0)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", &page(&[row(0), row(1)], None)), false).await?;
    let client = AlpacaDaily::for_loopback(config(schedule)?, &server.url)?;
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    ledger.lose_start.store(true, Ordering::SeqCst);
    assert!(s.fetch_page(&cancel).await.is_err());
    assert!(s.confirm_page(&cancel).await.is_err());
    assert!(s.fetch_page(&cancel).await.is_err());
    assert_eq!(server.count()?, 0);
    let cancel = Cancellation::default();
    let mut s = client
        .start(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    cancel.cancel();
    assert!(s.fetch_page(&cancel).await.is_err());
    assert_eq!(server.count()?, 0);
    Ok(())
}
