use super::*;
mod alpaca;
mod metered;
mod support;
mod twelve_data;
use edgeagent_market_data_application::{Cancellation, ProviderFailure as P};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::time::Duration;
use support::*;

#[test]
fn exact_json_tokens_retain_precision_and_reject_ambiguous_or_excess_values() -> TestResult {
    #[derive(Deserialize)]
    struct Row<'a> {
        #[serde(borrow)]
        price: &'a RawValue,
        #[serde(borrow)]
        volume: &'a RawValue,
    }
    let document = JsonDocument::parse(
        br#"{"price":9007199254740993.12345678,"volume":"1234567890123456789012345678"}"#,
    )?;
    let spaced = b" \n {\"price\":1.2500} \r\n";
    assert_eq!(JsonDocument::parse(spaced)?.bytes(), spaced);
    let row: Row<'_> = serde_json::from_str(document.root().get())?;
    assert_eq!(
        price(row.price)?.decimal_parts(),
        (900719925474099312345678, 8)
    );
    assert_eq!(
        volume(row.volume)?.decimal_parts(),
        (1234567890123456789012345678, 0)
    );
    for (token, expected) in [
        ("1.25e2", (125, 0)),
        ("125e-2", (125, 2)),
        ("\"1.2500\"", (125, 2)),
        ("1e-8", (1, 8)),
    ] {
        assert_eq!(
            price(serde_json::from_str(token)?)?.decimal_parts(),
            expected
        );
    }
    for bad in [
        "1e-9",
        "-1",
        "0",
        "1e128",
        "1e99999999999",
        "12345678901234567890123456789",
        "\"NaN\"",
        "null",
        "true",
        "\"01\"",
    ] {
        assert_eq!(
            price(serde_json::from_str(bad)?)
                .err()
                .ok_or("invalid price accepted")?
                .kind(),
            HttpErrorKind::InvalidJson
        );
    }
    assert_eq!(volume(serde_json::from_str("0")?)?.decimal_parts(), (0, 0));
    assert!(volume(serde_json::from_str("0.0000001")?).is_err());
    for bad in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"nested":[{"a":1,"\u0061":2}]}"#.as_slice(),
        b"{\"SENTINEL-SECRET\":".as_slice(),
        b"{} trailing".as_slice(),
        &[0xff],
    ] {
        let error = JsonDocument::parse(bad)
            .err()
            .ok_or("invalid JSON accepted")?;
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
    }
    let nested = format!("{}0{}", "[".repeat(34), "]".repeat(34));
    assert!(JsonDocument::parse(nested.as_bytes()).is_err());
    let nodes = format!("[{}0]", "0,".repeat(65_536));
    assert!(JsonDocument::parse(nodes.as_bytes()).is_err());
    assert!(JsonDocument::parse(&vec![b' '; 1_048_577]).is_err());
    assert!(!format!("{document:?}").contains("9007199"));
    Ok(())
}

#[tokio::test]
async fn endpoint_authority_credentials_and_redirects_cannot_expand_egress() -> TestResult {
    let server = Server::start(response(200, "", b"{}"), false).await?;
    let f = Fixture::new(1)?;
    let cancel = Cancellation::default();
    let admitted = f.admit(&cancel)?;
    assert!(HttpTransport::new(f.config(&server.url)?).is_err());
    for endpoint in [
        "http://example.com/data",
        "https://u:p@example.com/data",
        "https://example.com/data#fragment",
        "https://example.com/data?api_key=SENTINEL-SECRET",
    ] {
        assert!(HttpTransport::new(f.config(endpoint)?).is_err());
    }
    let client = f.client(&server.url)?;
    assert!(HttpTransport::for_loopback(f.config("https://external.invalid/data")?).is_err());
    let mut session = client.start(&admitted, at(NOW)?)?;
    for next in [
        "https://other.invalid/data?symbol=INVENTED&cursor=x",
        "/admin?symbol=INVENTED&cursor=x",
        "/data?symbol=OTHER&cursor=x",
        "/data?symbol=INVENTED&cursor=x&scope=all",
        "/data?symbol=INVENTED&cursor=x&cursor=y",
        "/data?symbol=INVENTED&cursor=x#secret",
    ] {
        assert_eq!(
            session
                .get(Some(next), &cancel)
                .await
                .err()
                .ok_or("expanded authority")?
                .kind(),
            HttpErrorKind::Authority
        );
    }
    assert_eq!(server.count()?, 0);
    assert_eq!(session.usage().attempts, 0);
    session.get(None, &cancel).await?;
    session
        .get(Some("?symbol=INVENTED&cursor=next"), &cancel)
        .await?;
    {
        let requests = server.requests.lock().map_err(|_| "fixture lock")?;
        assert_eq!(requests.len(), 2);
        let text = String::from_utf8_lossy(requests.first().ok_or("first request")?);
        assert!(
            text.to_ascii_lowercase()
                .contains("authorization: bearer invented-local-token")
        );
        assert!(
            text.to_ascii_lowercase()
                .contains("accept-encoding: identity")
        );
    }
    assert!(!format!("{client:?} {:?}", f.config(&server.url)?).contains("invented-local-token"));
    let foreign = Fixture::new(2)?;
    assert!(client.start(&foreign.admit(&cancel)?, at(NOW)?).is_err());
    let redirect = Server::start(
        response(302, &format!("Location: {}\r\n", server.url), b"{}"),
        false,
    )
    .await?;
    let redirect_client = f.client(&redirect.url)?;
    let mut s = redirect_client.start(&admitted, at(NOW)?)?;
    assert!(s.get(None, &cancel).await.is_err());
    assert_eq!(redirect.count()?, 1);
    assert_eq!(server.count()?, 2);
    Ok(())
}

#[tokio::test]
async fn response_streams_and_local_attempt_budgets_fail_closed_at_boundaries() -> TestResult {
    let mut f = Fixture::new(1)?;
    f.request.budget.response_bytes = nz(8)?;
    f.request.budget.command_bytes = nz(8)?;
    let cancel = Cancellation::default();
    let admitted = f.admit(&cancel)?;
    for wire in [response(200,"",b"123456789"),b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n9\r\n123456789\r\n0\r\n\r\n".to_vec()] {
        let server=Server::start(wire,false).await?;let client=f.client(&server.url)?;let mut session=client.start(&admitted,at(NOW)?)?;
        assert_eq!(session.get(None,&cancel).await.err().ok_or("oversized body accepted")?.kind(),HttpErrorKind::Budget);
    }
    let mut f = Fixture::new(1)?;
    f.request.budget.response_bytes = nz(4)?;
    f.request.budget.command_bytes = nz(4)?;
    let admitted = f.admit(&cancel)?;
    let server = Server::start(response(200, "", b"1234"), false).await?;
    let client = f.client(&server.url)?;
    let mut session = client.start(&admitted, at(NOW)?)?;
    session.get(None, &cancel).await?;
    assert_eq!(session.usage().observed_bytes, 4);
    assert_eq!(
        session
            .get(None, &cancel)
            .await
            .err()
            .ok_or("spent bytes reused")?
            .kind(),
        HttpErrorKind::Budget
    );
    assert_eq!(server.count()?, 1);
    let mut f = Fixture::new(1)?;
    f.request.budget.calls = nz(1)?;
    f.request.budget.pages = nz(1)?;
    let admitted = f.admit(&cancel)?;
    let client = f.client(&server.url)?;
    let mut session = client.start(&admitted, at(NOW)?)?;
    session.get(None, &cancel).await?;
    assert!(session.get(None, &cancel).await.is_err());
    assert_eq!(session.usage().attempts, 1);
    let broken=Server::start(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".to_vec(),false).await?;
    let client = f.client(&broken.url)?;
    assert!(
        client
            .start(&admitted, at(NOW)?)?
            .get(None, &cancel)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_original_deadlines_and_dropped_attempts_do_not_restart_work() -> TestResult {
    let f = Fixture::new(1)?;
    let cancel = Cancellation::default();
    let admitted = f.admit(&cancel)?;
    let server = Server::start(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n"
            .to_vec(),
        true,
    )
    .await?;
    let client = f.client(&server.url)?;
    for (advance, cancel_now, expected) in [
        (10, true, HttpErrorKind::Cancelled),
        (1100, false, HttpErrorKind::Provider(P::Timeout)),
        (5100, false, HttpErrorKind::Deadline),
    ] {
        let cancel = Cancellation::default();
        let mut session = client.start(&admitted, at(NOW)?)?;
        let call = session.get(None, &cancel);
        tokio::pin!(call);
        tokio::select! { result=&mut call=>return Err(format!("request completed before fixture barrier: {result:?}").into()), ()=server.received.notified()=>{} }
        tokio::time::pause();
        if cancel_now {
            cancel.cancel();
        }
        tokio::time::advance(Duration::from_millis(advance)).await;
        assert_eq!(
            call.await.err().ok_or("stalled body accepted")?.kind(),
            expected
        );
        tokio::time::resume();
    }
    let cancel = Cancellation::default();
    let mut session = client.start(&admitted, at(NOW)?)?;
    {
        let call = session.get(None, &cancel);
        tokio::pin!(call);
        tokio::select! {result=&mut call=>return Err(format!("unexpected completion: {result:?}").into()),()=server.received.notified()=>{}}
    }
    assert_eq!(session.usage().attempts, 1);
    cancel.cancel();
    assert_eq!(
        session
            .get(None, &cancel)
            .await
            .err()
            .ok_or("cancelled request started")?
            .kind(),
        HttpErrorKind::Cancelled
    );
    assert!(client.start(&admitted, at(NOW + 5000)?).is_err());
    let mut expired = client.start(&admitted, at(NOW)?)?;
    let before = server.count()?;
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(5001)).await;
    assert_eq!(
        expired
            .get(None, &Cancellation::default())
            .await
            .err()
            .ok_or("original deadline reset")?
            .kind(),
        HttpErrorKind::Deadline
    );
    assert_eq!(server.count()?, before);
    assert_eq!(expired.usage().attempts, 0);
    tokio::time::resume();
    Ok(())
}

#[tokio::test]
async fn statuses_retry_after_encodings_and_public_diagnostics_are_explicit() -> TestResult {
    let f = Fixture::new(1)?;
    let cancel = Cancellation::default();
    let admitted = f.admit(&cancel)?;
    // Valid gzip for invented {}, not an invalid compressed body that could mask an enabled decoder.
    let gzip = [
        0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 3, 0xab, 0xae, 5, 0, 0x43, 0xbf, 0xa6, 0xa3, 2, 0, 0, 0,
    ];
    let encoded = Server::start(response(200, "Content-Encoding: gzip\r\n", &gzip), false).await?;
    let encoded_client = f.client(&encoded.url)?;
    let mut encoded_session = encoded_client.start(&admitted, at(NOW)?)?;
    assert_eq!(
        encoded_session
            .get(None, &cancel)
            .await
            .err()
            .ok_or("implicit gzip accepted")?
            .kind(),
        HttpErrorKind::Provider(P::InvalidResponse)
    );
    assert_eq!(encoded_session.usage().observed_bytes, 0);
    for (status, headers, expected) in [
        (401, "", HttpErrorKind::Provider(P::Forbidden)),
        (403, "", HttpErrorKind::Provider(P::Forbidden)),
        (
            429,
            "Retry-After: 1\r\n",
            HttpErrorKind::Provider(P::Transient {
                retry_after_ms: Some(1000),
            }),
        ),
        (
            503,
            "",
            HttpErrorKind::Provider(P::Transient {
                retry_after_ms: None,
            }),
        ),
        (
            429,
            "Retry-After: 999999999999999999999\r\n",
            HttpErrorKind::Budget,
        ),
        (
            429,
            "Retry-After: not-a-date\r\n",
            HttpErrorKind::Provider(P::InvalidResponse),
        ),
        (
            200,
            "Content-Encoding: gzip\r\n",
            HttpErrorKind::Provider(P::InvalidResponse),
        ),
    ] {
        let server = Server::start(
            response(status, headers, b"{\"private\":\"SENTINEL-SECRET\"}"),
            false,
        )
        .await?;
        let client = f.client(&server.url)?;
        let mut session = client.start(&admitted, at(NOW)?)?;
        let error = session
            .get(None, &cancel)
            .await
            .err()
            .ok_or("invalid response accepted")?;
        assert_eq!(error.kind(), expected);
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        assert_eq!(server.count()?, 1);
    }
    let server = Server::start(
        response(429, "Retry-After: Sat, 03 Jan 1970 00:00:02 GMT\r\n", b"{}"),
        false,
    )
    .await?;
    let client = f.client(&server.url)?;
    let mut s = client.start(&admitted, at(NOW)?)?;
    let kind = s
        .get(None, &cancel)
        .await
        .err()
        .ok_or("transient succeeded")?
        .kind();
    assert!(
        matches!(kind,HttpErrorKind::Provider(P::Transient{retry_after_ms:Some(ms)})if (1..=2000).contains(&ms))
    );
    Ok(())
}

#[tokio::test]
async fn tls_handshake_failure_never_downgrades_or_exposes_request_secrets() -> TestResult {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let endpoint = format!("https://{}/SENTINEL-SECRET", listener.local_addr()?);
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await
    });
    let f = Fixture::new(1)?;
    let cancel = Cancellation::default();
    let admitted = f.admit(&cancel)?;
    let client = HttpTransport::new(f.config(&endpoint)?)?;
    let error = client
        .start(&admitted, at(NOW)?)?
        .get(None, &cancel)
        .await
        .err()
        .ok_or("plaintext accepted over TLS")?;
    assert!(!format!("{error:?} {error}").contains("SENTINEL"));
    if let Some(source) = std::error::Error::source(&error) {
        assert!(!format!("{source:?}").contains("SENTINEL"));
    }
    peer.abort();
    Ok(())
}
