use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_replay::*;
use std::sync::atomic::{AtomicUsize, Ordering};
#[path = "support/selection.rs"]
mod fixtures;
#[path = "../../market-data-application/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;
use fixtures::*;
use support::{Fixture, ready};

fn config(members: &[u128]) -> Result<(Fixture, SelectionFixture)> {
    let f = Fixture::new()?;
    let ids = members
        .iter()
        .map(|i| InstrumentId::new(*i))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let c = SelectionFixture::new(&f.request, &ids)?;
    Ok((f, c))
}
fn data(c: &SelectionFixture, seq: Option<u64>, name: &str) -> Result<FrameMetadata> {
    Ok(c.header(seq, FrameKind::Data(symbol(name)?)))
}
fn no_decode(_: InstrumentRoute<'_>, _: &[u8]) -> std::result::Result<(), SelectionError> {
    Err(SelectionError::Payload)
}

#[test]
fn latest_known_revisions_never_resurrect_obsolete_selection_or_directories() -> Result {
    let (f, mut c) = config(&[1])?;
    let old = c
        .selections
        .at(c.context.cutoff)
        .next()
        .ok_or("selection")?
        .clone();
    let effective = EffectiveInterval::new(
        time(0)?,
        time(f.request.admitted_at.unix_millis() + 86400000)?,
    )?;
    let future = time(c.context.cutoff.unix_millis() + 1)?;
    c.selections = ReferenceHistory::new(vec![
        old.clone(),
        version(
            1,
            2,
            future,
            c.context.scope.source,
            InstrumentSelection::new(effective, &[])?,
        )?,
    ])?;
    assert!(c.plan()?.prepare(&f.request).is_ok());
    c.context.cutoff = future;
    let plan = c.plan()?;
    let mut router = EarlyRouter::new(&plan, ContinuityMode::Snapshots)?;
    assert!(matches!(
        router.route(&data(&c, None, "INVENTED-A")?)?,
        RouteDecision::Skip
    ));
    c.context.cutoff = f.request.admitted_at;
    let moved = EffectiveInterval::new(future, time(future.unix_millis() + 1000)?)?;
    c.selections = ReferenceHistory::new(vec![
        old.clone(),
        version(
            1,
            2,
            c.context.cutoff,
            c.context.scope.source,
            InstrumentSelection::new(moved, &[InstrumentId::new(1)?])?,
        )?,
    ])?;
    assert_eq!(
        c.plan()
            .err()
            .ok_or("obsolete selection resurrected")?
            .to_string(),
        SelectionError::Revision.to_string()
    );
    c.selections = ReferenceHistory::new(vec![
        old.clone(),
        version(
            1,
            2,
            c.context.cutoff,
            SourceId::new(2)?,
            old.value().clone(),
        )?,
    ])?;
    assert!(matches!(c.plan(),Err(e) if e.to_string()==SelectionError::Scope.to_string()));
    c.selections = ReferenceHistory::new(vec![old])?;
    let previous = c
        .directories
        .at(c.context.cutoff)
        .next()
        .ok_or("directory")?
        .clone();
    c.directories = ReferenceHistory::new(vec![
        previous,
        version(
            2,
            2,
            c.context.cutoff,
            c.context.scope.source,
            RoutingDirectory::new(c.context.scope, effective, vec![])?,
        )?,
    ])?;
    assert!(c.plan().is_err()); // Do not revive a mapping removed by an atomic directory revision.
    Ok(())
}

struct CountingProvider(AtomicUsize);
impl DailyProvider for CountingProvider {
    async fn fetch(
        &self,
        r: &AuthorizedAcquisition<'_>,
        _: &Cancellation,
    ) -> std::result::Result<ProviderBatch, ProviderFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ProviderBatch {
            request: r.request().id,
            provider: r.authority().1.value().capability,
            observations: vec![],
        })
    }
}
#[test]
fn narrowed_requests_fail_before_provider_invocation_and_keep_exact_aliases() -> Result {
    let provider = CountingProvider(AtomicUsize::new(0));
    for members in [vec![], vec![2], vec![1]] {
        let (f, c) = config(&members)?;
        let cancel = Cancellation::default();
        let admitted = f
            .catalogs()
            .admit(&f.request, f.request.admitted_at, &cancel)?;
        let plan = c.plan()?;
        let selected = SelectedDailyProvider::new(&plan, &provider);
        let result = ready(selected.fetch(&admitted, &cancel))?;
        if members == [1] {
            result?;
            let narrowed = plan.prepare(&f.request)?;
            assert_eq!(
                narrowed.route().mapping().value().binding().0.as_str(),
                "INVENTED-A"
            );
            let mut foreign = f.request;
            foreign.scope.source = SourceId::new(2)?;
            assert!(plan.prepare(&foreign).is_err());
            let mut longer = f.request;
            longer.range = DateRange::new(longer.range.bounds().0, SessionDate::new(1970, 1, 5)?)?;
            assert!(plan.prepare(&longer).is_err());
            cancel.cancel();
            assert_eq!(
                ready(selected.fetch(&admitted, &cancel))?,
                Err(ProviderFailure::Cancelled)
            );
        } else {
            assert_eq!(result, Err(ProviderFailure::Unsupported));
            assert_eq!(provider.0.load(Ordering::SeqCst), 0);
        }
    }
    assert_eq!(provider.0.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn framing_and_header_selection_run_before_full_payload_decoding() -> Result {
    let (_, c) = config(&[1])?;
    let plan = c.plan()?;
    let mut router = EarlyRouter::new(&plan, ContinuityMode::Contiguous { next: 1 })?;
    let skipped = synthetic_frame(&data(&c, Some(1), "INVENTED-B")?, &[0xff, 0, 0xfe])?;
    assert_eq!(
        route_synthetic_frame(&mut router, &skipped, no_decode)?,
        RoutedFrame::Skipped
    );
    let heartbeat = synthetic_frame(&c.header(Some(2), FrameKind::Heartbeat), b"")?;
    assert_eq!(
        route_synthetic_frame(&mut router, &heartbeat, no_decode)?,
        RoutedFrame::Heartbeat
    );
    let calls = AtomicUsize::new(0);
    let selected = synthetic_frame(&data(&c, Some(3), "INVENTED-A")?, b"selected")?;
    let result = route_synthetic_frame(&mut router, &selected, |mapping, body| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(mapping.mapping().value().identities().0.get(), 1);
        assert_eq!(body, b"selected");
        Ok(body.len())
    })?;
    assert_eq!(result, RoutedFrame::Data(8));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let locate = synthetic_frame(
        &c.header(
            Some(4),
            FrameKind::Data(RoutingKey::locate(
                std::num::NonZeroU32::new(1).ok_or("locate")?,
            )),
        ),
        b"located",
    )?;
    assert_eq!(
        route_synthetic_frame(&mut router, &locate, |mapping, body| {
            assert_eq!(mapping.mapping().value().identities().0.get(), 1);
            Ok(body.len())
        })?,
        RoutedFrame::Data(7)
    );
    assert!(router.is_valid());
    let mut bad = skipped;
    bad.pop();
    assert_eq!(
        route_synthetic_frame(&mut router, &bad, no_decode),
        Err(SelectionError::Framing)
    );
    assert!(!router.is_valid());
    Ok(())
}

#[test]
fn controls_gaps_unknown_keys_and_stale_epochs_cannot_hide_behind_filtering() -> Result {
    let (_, c) = config(&[])?;
    let plan = c.plan()?;
    for (kind, error) in [
        (FrameKind::Reset, SelectionError::Reset),
        (
            FrameKind::DirectoryChanged,
            SelectionError::DirectoryChanged,
        ),
        (
            FrameKind::AuthenticationFailure,
            SelectionError::Authentication,
        ),
        (FrameKind::Data(symbol("UNKNOWN")?), SelectionError::Mapping),
    ] {
        let mut router = EarlyRouter::new(&plan, ContinuityMode::Contiguous { next: 1 })?;
        let frame = synthetic_frame(&c.header(Some(1), kind), b"")?;
        assert_eq!(
            route_synthetic_frame(&mut router, &frame, no_decode),
            Err(error)
        );
        assert!(!router.is_valid());
        assert!(matches!(
            router.route(&data(&c, Some(2), "INVENTED-B")?),
            Err(SelectionError::Invalidated)
        ));
    }
    for sequence in [None, Some(0), Some(2), Some(u64::MAX)] {
        let mut router = EarlyRouter::new(&plan, ContinuityMode::Contiguous { next: 1 })?;
        assert!(matches!(
            router.route(&data(&c, sequence, "INVENTED-B")?),
            Err(SelectionError::Continuity)
        ));
    }
    let mut router = EarlyRouter::new(&plan, ContinuityMode::Contiguous { next: 1 })?;
    assert!(matches!(
        router.route(&data(&c, Some(1), "INVENTED-B")?)?,
        RouteDecision::Skip
    ));
    assert!(matches!(
        router.route(&data(&c, Some(1), "INVENTED-B")?),
        Err(SelectionError::Continuity)
    ));
    for dimension in 0..3 {
        let mut router = EarlyRouter::new(&plan, ContinuityMode::Snapshots)?;
        let mut stale = data(&c, None, "INVENTED-B")?;
        match dimension {
            0 => stale.scope.source = SourceId::new(2)?,
            1 => stale.scope.feed = FeedId::new(2)?,
            _ => stale.scope.epoch = FeedEpoch::new(2)?,
        }
        assert!(matches!(router.route(&stale), Err(SelectionError::Scope)));
    }
    let mut router = EarlyRouter::new(&plan, ContinuityMode::Contiguous { next: u64::MAX })?;
    assert!(matches!(
        router.route(&data(&c, Some(u64::MAX), "INVENTED-B")?),
        Err(SelectionError::Continuity)
    ));
    Ok(())
}

#[test]
fn canonical_binding_tracks_content_revisions_and_order_without_diagnostic_leaks() -> Result {
    let (f, a) = config(&[2, 1])?;
    let (_, b) = config(&[1, 2])?;
    assert_eq!(a.plan()?.binding_bytes()?, b.plan()?.binding_bytes()?);
    let (_, mut changed) = config(&[1])?;
    assert_ne!(a.plan()?.binding_bytes()?, changed.plan()?.binding_bytes()?);
    let old = changed
        .selections
        .at(changed.context.cutoff)
        .next()
        .ok_or("selection")?
        .clone();
    let before = changed.plan()?.binding_bytes()?;
    changed.selections = ReferenceHistory::new(vec![version(
        1,
        2,
        time(0)?,
        changed.context.scope.source,
        old.value().clone(),
    )?])?;
    assert_ne!(before, changed.plan()?.binding_bytes()?);
    let renamed = SelectionFixture::with_alias(&f.request, &[InstrumentId::new(1)?], "RENAMED")?;
    let original = SelectionFixture::new(&f.request, &[InstrumentId::new(1)?])?;
    // The witnesses/checksums are identical; changed actual mapping content must still bind.
    assert_ne!(
        renamed.plan()?.binding_bytes()?,
        original.plan()?.binding_bytes()?
    );
    let plan = changed.plan()?;
    let request = plan.prepare(&f.request)?;
    let diagnostics = format!(
        "{plan:?} {request:?} {:?} {:?} {:?} {}",
        request.route(),
        changed.context,
        data(&changed, None, "INVENTED-A")?,
        SelectionError::Mapping
    );
    for secret in ["INVENTED-A", "INVENTED-B"] {
        assert!(!diagnostics.contains(secret));
    }
    assert!(!format!("{:?}", RoutedFrame::Data("sensitive-payload")).contains("sensitive-payload"));
    assert!(plan.binding_bytes()?.len() < 1_048_576);
    Ok(())
}

#[test]
fn bounds_ambiguous_mappings_and_malformed_frames_fail_closed() -> Result {
    let (f, c) = config(&[1])?;
    let effective =
        EffectiveInterval::new(time(0)?, time(f.request.admitted_at.unix_millis() + 1000)?)?;
    let many = (1..=33)
        .map(InstrumentId::new)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(
        InstrumentSelection::new(effective, &many),
        Err(SelectionError::Capacity)
    );
    assert!(InstrumentSelection::new(effective, &[many[0], many[0]]).is_err());
    let mapping = version(
        3,
        1,
        time(0)?,
        c.context.scope.source,
        InstrumentMapping::new(
            InstrumentId::new(1)?,
            ListingId::new(1)?,
            Symbol::new("INVENTED-A")?,
            Venue::Xnys,
            f.request.range,
        ),
    )?;
    let entry = RouteBinding::new(symbol("INVENTED-A")?, mapping.clone())?;
    assert_eq!(
        RoutingDirectory::new(c.context.scope, effective, vec![entry.clone(); 4097]),
        Err(SelectionError::Capacity)
    );
    assert!(RoutingDirectory::new(c.context.scope, effective, vec![entry.clone(), entry]).is_err());
    assert!(RouteBinding::new(symbol("CONTRADICTION")?, mapping).is_err());
    let plan = c.plan()?;
    for mutation in 0..4 {
        let mut frame = synthetic_frame(&data(&c, None, "INVENTED-B")?, b"x")?;
        match mutation {
            0 => frame[0] = 0,
            1 => frame[4] = 255,
            2 => {
                frame.pop();
            }
            _ => frame.push(0),
        }
        let mut router = EarlyRouter::new(&plan, ContinuityMode::Snapshots)?;
        assert_eq!(
            route_synthetic_frame(&mut router, &frame, no_decode),
            Err(SelectionError::Framing)
        );
        assert!(!router.is_valid());
    }
    assert!(synthetic_frame(&data(&c, None, "INVENTED-A")?, &vec![0; 1048576]).is_err());
    assert!(synthetic_frame(&c.header(None, FrameKind::Heartbeat), b"not-control-data").is_err());
    let mut golden_header = c.header(
        Some(7),
        FrameKind::Data(RoutingKey::locate(
            std::num::NonZeroU32::new(2).ok_or("locate")?,
        )),
    );
    golden_header.scope.feed = FeedId::new(9)?;
    golden_header.scope.epoch = FeedEpoch::new(4)?;
    let mut golden = b"EAS1\x01".to_vec();
    golden.extend(1_u128.to_be_bytes());
    golden.extend(9_u128.to_be_bytes());
    golden.extend(4_u64.to_be_bytes());
    golden.push(1);
    golden.extend(7_u64.to_be_bytes());
    golden.extend(2_u32.to_be_bytes());
    golden.extend(1_u32.to_be_bytes());
    golden.push(b'x');
    assert_eq!(synthetic_frame(&golden_header, b"x")?, golden);
    let mut router = EarlyRouter::new(&plan, ContinuityMode::Snapshots)?;
    let frame = synthetic_frame(&data(&c, None, "INVENTED-A")?, b"invalid")?;
    assert_eq!(
        route_synthetic_frame(&mut router, &frame, no_decode),
        Err(SelectionError::Payload)
    );
    assert!(!router.is_valid());
    Ok(())
}
