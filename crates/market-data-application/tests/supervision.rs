use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::num::NonZeroU16;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn n(value: u16) -> TestResult<NonZeroU16> {
    Ok(NonZeroU16::new(value).ok_or("zero fixture bound")?)
}
fn epoch() -> TestResult<FeedEpoch> {
    Ok(FeedEpoch::new(1)?)
}
fn provider(source: u128) -> TestResult<ProviderCapability> {
    let rules = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
    Ok(ProviderCapability::new(
        SourceId::new(source)?,
        EvidenceContract::new(
            EvidenceKind::IndexLevel,
            MarketScope::IndexPublisher,
            rules,
            TimestampPrecision::Nanoseconds,
        )?,
        DeliveryCapability::Snapshots,
        KnowledgeCapability::CaptureOnly,
    ))
}
fn lane(feed: u128, capacity: u16) -> TestResult<SupervisedSource> {
    Ok(SupervisedSource::new(
        FeedId::new(feed)?,
        epoch()?,
        provider(feed)?,
        n(capacity)?,
    )?)
}
fn observation(source: u128, id: u128) -> TestResult<CanonicalObservation> {
    let at = PreciseInstant::from_millis(UtcInstant::from_unix_millis(0)?);
    Ok(CanonicalObservation::new(
        ObservationContext::new(
            InstrumentId::new(1)?,
            provider(source)?,
            TransformationRevision::new(1)?,
            SessionDate::new(1970, 1, 1)?,
            &[],
        )?,
        ObservationVersion::new(ObservationId::new(id)?, ObservationRevision::new(1)?, None)?,
        EvidenceTimes::new(at, None, None, at)?,
        None,
        Some(ObservationValue::IndexLevel(IndexPoints::new(
            987_654_321,
            0,
        )?)),
    )?)
}
fn enqueue(supervisor: &mut StreamSupervisor, feed: u128, id: u128) -> TestResult {
    supervisor.enqueue(FeedId::new(feed)?, epoch()?, observation(feed, id)?)?;
    Ok(())
}

#[test]
fn supervision_round_robin_preserves_fifo_and_bounds_every_eligible_wait() -> TestResult {
    // Saturate every portable lane, then keep the first lane continuously hot.
    let sources = (1..=32)
        .map(|feed| lane(feed, 2))
        .collect::<TestResult<Vec<_>>>()?;
    let mut supervisor = StreamSupervisor::new(&sources, n(64)?, Cancellation::default())?;
    for feed in 1..=32 {
        enqueue(&mut supervisor, feed, 1)?;
        enqueue(&mut supervisor, feed, 2)?;
    }
    for round in 1..=2 {
        for feed in 1..=32 {
            let work = supervisor.dispatch_next()?.ok_or("missing turn")?;
            assert_eq!(work.identity(), (FeedId::new(feed)?, epoch()?));
            assert_eq!(work.observation(), &observation(feed, round)?);
            supervisor.confirm(work)?;
            if feed == 1 {
                enqueue(&mut supervisor, feed, round + 2)?;
            }
        }
    }
    supervisor.check_health()?;
    Ok(())
}

#[test]
fn supervision_busy_and_failed_sources_cannot_starve_other_lanes() -> TestResult {
    let mut supervisor = StreamSupervisor::new(
        &[lane(1, 2)?, lane(2, 2)?, lane(3, 2)?],
        n(6)?,
        Cancellation::default(),
    )?;
    for feed in 1..=3 {
        enqueue(&mut supervisor, feed, 1)?;
    }
    let first = supervisor.dispatch_next()?.ok_or("first")?;
    assert_eq!(supervisor.check_health(), Err(SupervisionError::Unresolved));
    supervisor.stop_source(FeedId::new(2)?, StreamStop::SourceFailure)?;
    let third = supervisor.dispatch_next()?.ok_or("third")?;
    assert_eq!(third.identity().0, FeedId::new(3)?);
    supervisor.confirm(third)?;
    enqueue(&mut supervisor, 3, 2)?;
    let again = supervisor.dispatch_next()?.ok_or("healthy lane starved")?;
    assert_eq!(again.identity().0, FeedId::new(3)?);
    supervisor.confirm(again)?;
    assert!(supervisor.dispatch_next()?.is_none());
    supervisor.confirm(first)?;
    assert_eq!(
        supervisor.check_health(),
        Err(SupervisionError::Stopped(StreamStop::SourceFailure))
    );
    assert_eq!(supervisor.status(FeedId::new(2)?)?.pending, 1);
    Ok(())
}

#[test]
fn supervision_overload_retains_rejected_and_unresolved_records_without_eviction() -> TestResult {
    // Exercise both independent ceilings; dispatched heads still occupy capacity.
    for (per_source, total) in [(1, 4), (4, 1)] {
        let mut supervisor =
            StreamSupervisor::new(&[lane(1, per_source)?], n(total)?, Cancellation::default())?;
        enqueue(&mut supervisor, 1, 1)?;
        let work = supervisor.dispatch_next()?.ok_or("head")?;
        let value = observation(1, 2)?;
        let rejected = supervisor
            .enqueue(FeedId::new(1)?, epoch()?, value.clone())
            .err()
            .ok_or("expected overload")?;
        assert_eq!(
            rejected.reason(),
            SupervisionError::Stopped(StreamStop::Overload)
        );
        assert_eq!(rejected.into_observation(), value);
        assert_eq!(
            supervisor.confirm(work),
            Err(SupervisionError::Stopped(StreamStop::Overload))
        );
        assert_eq!(
            supervisor.status(FeedId::new(1)?)?,
            StreamLaneStatus {
                pending: 1,
                unresolved: true,
                stopped: Some(StreamStop::Overload),
            }
        );
        assert!(supervisor.dispatch_next()?.is_none());
        assert_eq!(
            supervisor.check_health(),
            Err(SupervisionError::Stopped(StreamStop::Overload))
        );
    }
    // Aggregate pressure crosses lane boundaries. Confirming another lane frees
    // occupancy but cannot clear the affected lane's latched evidence incident.
    let mut shared =
        StreamSupervisor::new(&[lane(1, 2)?, lane(2, 2)?], n(1)?, Cancellation::default())?;
    enqueue(&mut shared, 1, 1)?;
    let first = shared.dispatch_next()?.ok_or("shared head")?;
    let rejected = shared
        .enqueue(FeedId::new(2)?, epoch()?, observation(2, 1)?)
        .err()
        .ok_or("aggregate overload")?;
    assert_eq!(rejected.into_observation(), observation(2, 1)?);
    shared.confirm(first)?;
    enqueue(&mut shared, 1, 2)?;
    assert_eq!(
        shared.status(FeedId::new(2)?)?.stopped,
        Some(StreamStop::Overload)
    );
    Ok(())
}

#[test]
fn supervision_foreign_and_dropped_dispatches_never_confirm_or_retry_heads() -> TestResult {
    let make = || -> TestResult<StreamSupervisor> {
        Ok(StreamSupervisor::new(
            &[lane(1, 2)?],
            n(2)?,
            Cancellation::default(),
        )?)
    };
    let mut first: StreamSupervisor = make()?;
    let mut second: StreamSupervisor = make()?;
    enqueue(&mut first, 1, 1)?;
    enqueue(&mut second, 1, 1)?;
    let foreign = first.dispatch_next()?.ok_or("foreign head")?;
    let dropped = second.dispatch_next()?.ok_or("local head")?;
    assert_eq!(
        second.confirm(foreign),
        Err(SupervisionError::ForeignDispatch)
    );
    drop(dropped);
    for supervisor in [&mut first, &mut second] {
        assert_eq!(supervisor.check_health(), Err(SupervisionError::Unresolved));
        assert!(supervisor.dispatch_next()?.is_none());
        assert_eq!(supervisor.status(FeedId::new(1)?)?.pending, 1);
    }
    Ok(())
}

#[test]
fn supervision_cancellation_and_source_failure_are_sticky_without_compensation() -> TestResult {
    let cancel = Cancellation::default();
    let mut supervisor = StreamSupervisor::new(&[lane(1, 2)?, lane(2, 2)?], n(4)?, cancel.clone())?;
    enqueue(&mut supervisor, 1, 1)?;
    enqueue(&mut supervisor, 2, 1)?;
    let work = supervisor.dispatch_next()?.ok_or("head")?;
    supervisor.stop_source(FeedId::new(1)?, StreamStop::SourceFailure)?;
    supervisor.stop_source(FeedId::new(1)?, StreamStop::Overload)?;
    cancel.cancel();
    assert_eq!(
        supervisor.check_health(),
        Err(SupervisionError::Stopped(StreamStop::Cancelled))
    );
    assert_eq!(
        supervisor.status(FeedId::new(2)?)?.stopped,
        Some(StreamStop::Cancelled)
    );
    assert_eq!(
        supervisor.confirm(work),
        Err(SupervisionError::Stopped(StreamStop::Cancelled))
    );
    assert_eq!(
        supervisor.dispatch_next().err(),
        Some(SupervisionError::Stopped(StreamStop::Cancelled))
    );
    let rejected = supervisor
        .enqueue(FeedId::new(2)?, epoch()?, observation(2, 2)?)
        .err()
        .ok_or("cancelled intake")?;
    assert_eq!(rejected.into_observation(), observation(2, 2)?);
    assert_eq!(
        supervisor.status(FeedId::new(1)?)?,
        StreamLaneStatus {
            pending: 1,
            unresolved: true,
            stopped: Some(StreamStop::SourceFailure),
        }
    );
    assert_eq!(supervisor.status(FeedId::new(2)?)?.pending, 1);
    Ok(())
}

#[test]
fn supervision_configuration_binding_and_diagnostics_fail_closed() -> TestResult {
    let cancellation = Cancellation::default();
    for sources in [
        vec![],
        vec![lane(1, 1)?, lane(1, 2)?],
        (1..=33)
            .map(|feed| lane(feed, 1))
            .collect::<TestResult<Vec<_>>>()?,
    ] {
        assert_eq!(
            StreamSupervisor::new(&sources, n(1)?, cancellation.clone()).err(),
            Some(SupervisionError::Configuration)
        );
    }
    assert_eq!(
        lane(1, 4097).err().map(|e| e.to_string()),
        Some(SupervisionError::Configuration.to_string())
    );
    assert_eq!(
        StreamSupervisor::new(&[lane(1, 1)?], n(4097)?, cancellation.clone()).err(),
        Some(SupervisionError::Configuration)
    );
    let mut supervisor = StreamSupervisor::new(&[lane(1, 4096)?], n(4096)?, cancellation)?;
    // Empty healthy scheduling is deliberately NOT evidence readiness.
    supervisor.check_health()?;
    assert!(supervisor.dispatch_next()?.is_none());
    let unknown = supervisor
        .enqueue(FeedId::new(2)?, epoch()?, observation(2, 1)?)
        .err()
        .ok_or("unknown source")?;
    assert_eq!(unknown.reason(), SupervisionError::UnknownSource);
    supervisor.check_health()?;
    let stale = supervisor
        .enqueue(FeedId::new(1)?, FeedEpoch::new(2)?, observation(1, 1)?)
        .err()
        .ok_or("epoch")?;
    assert_eq!(
        stale.reason(),
        SupervisionError::Stopped(StreamStop::Binding)
    );
    assert_eq!(format!("{supervisor:?}"), "StreamSupervisor(..)");
    assert_eq!(format!("{stale:?}"), "RejectedStreamObservation(..)");
    assert!(!format!("{stale}").contains("987654321"));
    let mut other = StreamSupervisor::new(&[lane(1, 1)?], n(1)?, Cancellation::default())?;
    let wrong = other
        .enqueue(FeedId::new(1)?, epoch()?, observation(2, 1)?)
        .err()
        .ok_or("provider")?;
    assert_eq!(
        wrong.reason(),
        SupervisionError::Stopped(StreamStop::Binding)
    );
    let mut valid = StreamSupervisor::new(&[lane(1, 2)?], n(2)?, Cancellation::default())?;
    // Duplicate arrivals keep identity/receipt and occupy separate slots; no implicit dedup.
    enqueue(&mut valid, 1, 1)?;
    enqueue(&mut valid, 1, 1)?;
    let dispatch = valid.dispatch_next()?.ok_or("dispatch")?;
    assert_eq!(format!("{dispatch:?}"), "StreamDispatch(..)");
    assert_eq!(format!("{:?}", lane(1, 1)?), "SupervisedSource(..)");
    valid.confirm(dispatch)?;
    assert_eq!(valid.status(FeedId::new(1)?)?.pending, 1);
    Ok(())
}
