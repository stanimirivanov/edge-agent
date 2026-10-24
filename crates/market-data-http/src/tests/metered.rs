use super::support::*;
use crate::*;
use edgeagent_contracts::evidence::{AcquireEvidence, EvidenceId};
use edgeagent_market_data_application::*;
use std::{sync::atomic::Ordering, time::Duration};
pub(super) mod support;
use support::*;

#[tokio::test(start_paused = true)]
async fn each_page_requires_confirmed_permit_and_usage_is_ledger_backed() -> TestResult {
    let f = fixture()?;
    let ledger = Ledger::new(&f)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", b"{}"), false).await?;
    let client = f.client(&server.url)?;
    let cancel = Cancellation::default();
    let traversal = traversal()?;
    let mut s = client
        .start_metered(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    assert_eq!(s.get_page(&traversal, None, &cancel).await?.bytes(), b"{}");
    let usage = s.usage().await?;
    assert_eq!(
        (
            usage.durable.calls,
            usage.durable.charged_bytes,
            usage.durable.pending
        ),
        (1, 2, false)
    );
    budget_kind(
        s.get_page(&traversal, None, &cancel).await,
        BudgetErrorKind::RateLimited,
    )?;
    assert_eq!(server.count()?, 1);
    ledger.set_time(&authority, NOW + 15000);
    tokio::time::advance(Duration::from_millis(15000)).await;
    s.get_page(&traversal, Some("?symbol=INVENTED&cursor=next"), &cancel)
        .await?;
    assert_eq!((s.usage().await?.durable.calls, server.count()?), (2, 2));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn lost_reservation_confirmation_and_inflight_drop_never_redispatch() -> TestResult {
    for fault in ["lost", "reservation-drop", "request-drop"] {
        let lost = fault == "lost";
        let f = fixture()?;
        let ledger = Ledger::new(&f)?;
        let authority = authority(&f);
        ledger.lose_start.store(lost, Ordering::SeqCst);
        ledger
            .pause_start
            .store(fault == "reservation-drop", Ordering::SeqCst);
        let server = Server::start(Vec::new(), true).await?;
        let client = f.client(&server.url)?;
        let cancel = Cancellation::default();
        let traversal = traversal()?;
        let mut s = client
            .start_metered(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        if lost {
            budget_kind(
                s.get_page(&traversal, None, &cancel).await,
                BudgetErrorKind::Storage,
            )?;
        } else if fault == "reservation-drop" {
            let page = s.get_page(&traversal, None, &cancel);
            tokio::pin!(page);
            tokio::select! {biased; _ = ledger.starting.notified()=>{},
            result=&mut page=>return Err(format!("unexpected: {result:?}").into())}
        } else {
            let page = s.get_page(&traversal, None, &cancel);
            tokio::pin!(page);
            tokio::select! {biased;_ = server.received.notified()=>{},result=&mut page=>return Err(format!("unexpected: {result:?}").into())}
        }
        budget_kind(
            s.get_page(&traversal, None, &cancel).await,
            BudgetErrorKind::Busy,
        )?;
        let mut restarted = client
            .start_metered(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        budget_kind(
            restarted.get_page(&traversal, None, &cancel).await,
            BudgetErrorKind::Busy,
        )?;
        let usage = s.usage().await?;
        assert_eq!(
            (
                usage.durable.calls,
                usage.durable.charged_bytes,
                usage.durable.pending
            ),
            (1, 1048576, true)
        );
        assert_eq!(server.count()?, usize::from(fault == "request-drop"));
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn completed_page_reconfirmation_survives_lost_or_dropped_confirmation() -> TestResult {
    for dropped in [false, true] {
        let f = fixture()?;
        let ledger = Ledger::new(&f)?;
        let authority = authority(&f);
        ledger.lose_complete.store(!dropped, Ordering::SeqCst);
        ledger.pause_complete.store(dropped, Ordering::SeqCst);
        let server = Server::start(response(200, "", b"{}"), false).await?;
        let client = f.client(&server.url)?;
        let cancel = Cancellation::default();
        let traversal = traversal()?;
        let mut s = client
            .start_metered(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        if dropped {
            let page = s.get_page(&traversal, None, &cancel);
            tokio::pin!(page);
            tokio::select! {biased;_=ledger.completing.notified()=>{},result=&mut page=>return Err(format!("unexpected: {result:?}").into())}
        } else {
            budget_kind(
                s.get_page(&traversal, None, &cancel).await,
                BudgetErrorKind::Storage,
            )?;
        }
        budget_kind(
            s.get_page(&traversal, None, &cancel).await,
            BudgetErrorKind::Busy,
        )?;
        assert!(s.usage().await?.awaiting_confirmation);
        ledger.pause_complete.store(false, Ordering::SeqCst);
        assert_eq!(s.confirm_page(&cancel).await?.bytes(), b"{}");
        assert_eq!(
            (s.usage().await?.durable.charged_bytes, server.count()?),
            (2, 1)
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn authority_cancellation_and_original_deadlines_are_rechecked_after_waits() -> TestResult {
    let f = fixture()?;
    let ledger = Ledger::new(&f)?;
    let authority = authority(&f);
    let server = Server::start(response(200, "", b"{}"), false).await?;
    let client = f.client(&server.url)?;
    let mut s = client
        .start_metered(&ledger, &ledger.record, &authority, &ledger.after_start)
        .await?;
    budget_kind(
        s.get_page(&traversal()?, None, &ledger.after_start).await,
        BudgetErrorKind::Cancelled,
    )?;
    assert_eq!(
        (
            s.usage().await?.durable.calls,
            s.usage().await?.durable.charged_bytes,
            server.count()?
        ),
        (1, 0, 0)
    );
    for expired in [false, true] {
        let ledger = Ledger::new(&f)?;
        let authority = support::authority(&f);
        let cancel = Cancellation::default();
        ledger.lose_complete.store(true, Ordering::SeqCst);
        let mut s = client
            .start_metered(&ledger, &ledger.record, &authority, &cancel)
            .await?;
        budget_kind(
            s.get_page(&traversal()?, None, &cancel).await,
            BudgetErrorKind::Storage,
        )?;
        if expired {
            ledger.set_time(&authority, NOW + 60000);
        } else {
            authority.deny.store(true, Ordering::SeqCst);
        }
        assert!(s.confirm_page(&cancel).await.is_err());
        assert!(!s.usage().await?.durable.pending);
        assert!(!s.usage().await?.awaiting_confirmation);
        budget_kind(s.confirm_page(&cancel).await, BudgetErrorKind::Busy)?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn partial_overflow_and_retry_after_accounting_stays_conservative() -> TestResult {
    for (reply,known,bytes) in [
        (b"HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: 20\r\n\r\n{".to_vec(),false,1048576),
        (response(200,"",&vec![b'x';1048577]),false,1048576),
        (response(429,"Retry-After: 120\r\n",b""),true,0),
        (response(200,"",b"invalid"),true,7),
    ] {
        let f=fixture()?;let ledger=Ledger::new(&f)?;let authority=authority(&f);
        let server=Server::start(reply,false).await?;let client=f.client(&server.url)?;let cancel=Cancellation::default();
        let mut s=client.start_metered(&ledger,&ledger.record,&authority,&cancel).await?;
        assert!(s.get_page(&traversal()?,None,&cancel).await.is_err());
        let usage=s.usage().await?;
        assert_eq!((usage.durable.charged_bytes,usage.durable.pending),(bytes,!known));
        assert_eq!(usage.durable.calls,1);
        if known && bytes==0 {budget_kind(s.get_page(&traversal()?,None,&cancel).await,BudgetErrorKind::Exhausted)?;}
        assert_eq!(server.count()?,1);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dispatch_diagnostics_and_usage_exclude_identity_and_payloads() -> TestResult {
    // Current-thread runtime: keep the thread-local recorder scoped over the actual dispatch.
    let recorder = Capture::default();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let f = fixture()?;
    let ledger = Ledger::new(&f)?;
    let authority = authority(&f);
    let cancel = Cancellation::default();
    let server = Server::start(response(200, "", b"{}"), false).await?;
    let client = f.client(&server.url)?;
    let mut s = client
        .start_metered(&ledger, &ledger.record, &authority, &cancel)
        .await?;
    s.get_page(&traversal()?, None, &cancel).await?;
    let check = authority.check(&ledger.record, &cancel)?;
    let error = DispatchError::Budget(BudgetError::storage(std::io::Error::other(
        "sensitive-source",
    )));
    let diagnostics = format!("{s:?} {check:?} {:?} {error:?} {error}", s.usage().await?);
    for secret in [
        "sensitive-source",
        "INVENTED",
        "invented-local-token",
        ledger.record.command().request.as_str(),
    ] {
        assert!(!diagnostics.contains(secret));
    }
    assert!(std::error::Error::source(&error).is_some());
    let keys = recorder.0.lock().map_err(|_| "recorder lock")?;
    assert_eq!(keys.len(), 3);
    for key in keys.iter() {
        let labels = key
            .labels()
            .map(|l| (l.key(), l.value()))
            .collect::<Vec<_>>();
        match key.name() {
            "edgeagent_provider_dispatch_total" => {
                assert!(labels == [("stage", "reserved")] || labels == [("stage", "confirmed")]);
            }
            "edgeagent_provider_observed_bytes_total" => assert!(labels.is_empty()),
            _ => return Err("unexpected metric".into()),
        }
    }
    Ok(())
}
