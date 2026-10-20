#[path = "continuity/support.rs"]
pub mod support;
use edgeagent_market_data_domain::*;
use std::time::Duration;
use support::*;
fn portable<T: Send + Sync>() {}

#[test]
fn readiness_requires_the_whole_pinned_plan_in_declared_order() -> TestResult {
    let kind = EvidenceKind::IndexLevel;
    let mut first = tracker(1, kind, ContinuityMode::Snapshots, 8, 3)?;
    let mut second = tracker(2, kind, ContinuityMode::Snapshots, 8, 3)?;
    first.ingest(epoch(1)?, time(5)?, index(None, 1, 5, 5)?)?;
    second.ingest(
        epoch(1)?,
        time(6)?,
        record(
            None,
            2,
            10,
            6,
            6,
            ObservationValue::IndexLevel(IndexPoints::new(100, 0)?),
        )?,
    )?;
    first.heartbeat(epoch(1)?, time(6)?, None)?;
    second.heartbeat(epoch(1)?, time(6)?, None)?;
    let requirements = [
        required(1, 9, kind, ContinuityRequirement::Snapshots)?,
        required(2, 10, kind, ContinuityRequirement::Snapshots)?,
    ];
    let policy = plan(&requirements, 5, 1)?;
    let inputs = [input(&second, 2, 1)?, input(&first, 1, 1)?];
    let ready = policy.check(time(10)?, &inputs)?;
    assert_eq!(ready.assessment(), (time(10)?, rules(2)?));
    assert_eq!(
        ready.feeds(),
        &[
            (FeedId::new(1)?, epoch(1)?, rules(1)?),
            (FeedId::new(2)?, epoch(1)?, rules(1)?)
        ]
    );
    assert_eq!(
        ready.feeds(),
        &[
            (FeedId::new(1)?, epoch(1)?, rules(1)?),
            (FeedId::new(2)?, epoch(1)?, rules(1)?)
        ]
    );
    let instruments: Vec<_> = ready
        .observations()
        .iter()
        .map(|row| row.context().origin().0.get())
        .collect();
    assert_eq!(instruments, vec![9, 10]);
    assert_eq!(format!("{ready:?}"), "ReadyEvidence { inputs: 2, .. }");
    assert!(matches!(
        policy.check(PreciseInstant::from_unix_parts(0, 10_000_001)?, &inputs),
        Err(ContinuityError::Stale)
    ));
    assert!(matches!(
        plan(&requirements, 5, 0)?.check(time(10)?, &inputs),
        Err(ContinuityError::Skew)
    ));
    assert!(matches!(
        policy.check(time(10)?, &[input(&first, 1, 1)?]),
        Err(ContinuityError::Input)
    ));
    assert!(matches!(
        policy.check(time(10)?, &[input(&first, 1, 1)?, input(&first, 1, 1)?]),
        Err(ContinuityError::Input)
    ));
    let wrong = plan(
        &[required(
            1,
            9,
            EvidenceKind::ShareTrade,
            ContinuityRequirement::Snapshots,
        )?],
        5,
        0,
    )?;
    assert!(matches!(
        wrong.check(time(10)?, &[input(&first, 1, 1)?]),
        Err(ContinuityError::Scope)
    ));
    let mut fork = tracker(1, kind, ContinuityMode::Snapshots, 8, 3)?;
    fork.ingest(
        epoch(1)?,
        time(6)?,
        record(
            None,
            2,
            10,
            6,
            6,
            ObservationValue::IndexLevel(IndexPoints::new(100, 0)?),
        )?,
    )?;
    fork.heartbeat(epoch(1)?, time(6)?, None)?;
    assert!(matches!(
        policy.check(time(10)?, &[input(&first, 1, 1)?, input(&fork, 2, 1)?]),
        Err(ContinuityError::Input)
    ));
    Ok(())
}

#[test]
fn snapshots_cannot_claim_continuity_and_repaired_gaps_need_explicit_completion() -> TestResult {
    let mut snapshots = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 8, 3)?;
    snapshots.ingest(epoch(1)?, time(0)?, index(None, 1, 0, 0)?)?;
    snapshots.heartbeat(epoch(1)?, time(0)?, None)?;
    let policy = plan(
        &[required(
            1,
            9,
            EvidenceKind::IndexLevel,
            ContinuityRequirement::Contiguous,
        )?],
        10,
        0,
    )?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&snapshots, 1, 1)?]),
        Err(ContinuityError::ContinuityUnknown)
    ));
    let (scope, _, _) = snapshots.identity();
    assert!(matches!(
        ContinuityTracker::new(
            scope,
            epoch(1)?,
            ContinuityMode::Contiguous { next: 0 },
            ContinuityPolicy::new(
                rules(1)?,
                8,
                3,
                Duration::from_millis(10),
                Duration::from_millis(20)
            )?,
            time(0)?
        ),
        Err(ContinuityError::ContinuityUnknown)
    ));
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        8,
        3,
    )?;
    feed.ingest(epoch(1)?, time(0)?, index(Some(0), 1, 0, 0)?)?;
    feed.ingest(epoch(1)?, time(0)?, index(Some(2), 3, 0, 0)?)?;
    feed.heartbeat(epoch(1)?, time(0)?, Some(2))?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Recovering)
    ));
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 3, 1)?]),
        Err(ContinuityError::Input)
    ));
    feed.ingest(epoch(1)?, time(0)?, index(Some(1), 2, 0, 0)?)?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 3, 1)?]),
        Err(ContinuityError::Recovering)
    ));
    let ticket = feed.begin_recovery(epoch(1)?, time(0)?)?;
    feed.finish_recovery(ticket, time(0)?)?;
    policy.check(time(0)?, &[input(&feed, 3, 1)?])?;
    Ok(())
}

#[test]
fn contiguous_readiness_requires_a_watermark_covering_the_accepted_prefix() -> TestResult {
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        8,
        3,
    )?;
    feed.ingest(epoch(1)?, time(0)?, index(Some(0), 1, 0, 0)?)?;
    feed.heartbeat(epoch(1)?, time(0)?, None)?;
    let policy = plan(
        &[required(
            1,
            9,
            EvidenceKind::IndexLevel,
            ContinuityRequirement::Contiguous,
        )?],
        10,
        0,
    )?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::ContinuityUnknown)
    ));
    feed.heartbeat(epoch(1)?, time(0)?, Some(0))?;
    policy.check(time(0)?, &[input(&feed, 1, 1)?])?;
    feed.ingest(epoch(1)?, time(0)?, index(Some(1), 2, 0, 0)?)?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 2, 1)?]),
        Err(ContinuityError::ContinuityUnknown)
    ));
    assert_eq!(
        feed.heartbeat(epoch(1)?, time(0)?, Some(0)),
        Err(ContinuityError::Watermark)
    );
    feed.heartbeat(epoch(1)?, time(0)?, Some(1))?;
    policy.check(time(0)?, &[input(&feed, 2, 1)?])?;
    Ok(())
}

#[test]
fn heartbeat_freshness_future_receipts_and_old_epochs_fail_closed() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 8, 3)?;
    let policy = plan(
        &[required(
            1,
            9,
            EvidenceKind::IndexLevel,
            ContinuityRequirement::Snapshots,
        )?],
        100,
        0,
    )?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(0)?, index(None, 1, 0, 1)?),
        Err(ContinuityError::Clock)
    );
    assert_eq!(feed.status().1, 0);
    feed.ingest(epoch(1)?, time(0)?, index(None, 1, 0, 0)?)?;
    assert!(matches!(
        policy.check(time(0)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::HeartbeatMissing)
    ));
    feed.heartbeat(epoch(1)?, time(0)?, None)?;
    policy.check(time(10)?, &[input(&feed, 1, 1)?])?;
    assert!(matches!(
        policy.check(
            PreciseInstant::from_unix_parts(0, 10_000_001)?,
            &[input(&feed, 1, 1)?]
        ),
        Err(ContinuityError::HeartbeatGap)
    ));
    feed.heartbeat(epoch(1)?, time(11)?, None)?;
    assert!(matches!(
        policy.check(time(11)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Recovering)
    ));
    feed.reset(epoch(2)?, ContinuityMode::Snapshots, time(11)?)?;
    let stale = EvidenceInput::new(
        &feed,
        epoch(1)?,
        ObservationId::new(1)?,
        ObservationRevision::new(1)?,
    );
    assert!(matches!(
        policy.check(time(11)?, &[stale]),
        Err(ContinuityError::Epoch)
    ));
    Ok(())
}

#[test]
fn superseded_and_cancelled_revisions_never_fall_back_to_an_old_price() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 8, 3)?;
    let original = index(None, 1, 0, 0)?;
    feed.ingest(epoch(1)?, time(0)?, original.clone())?;
    feed.ingest(
        epoch(1)?,
        time(1)?,
        revision(&original, 2, 1, None, 1, original.value())?,
    )?;
    feed.heartbeat(epoch(1)?, time(1)?, None)?;
    let policy = plan(
        &[required(
            1,
            9,
            EvidenceKind::IndexLevel,
            ContinuityRequirement::Snapshots,
        )?],
        10,
        0,
    )?;
    assert!(matches!(
        policy.check(time(1)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Superseded)
    ));
    policy.check(time(1)?, &[input(&feed, 1, 2)?])?;
    feed.ingest(
        epoch(1)?,
        time(2)?,
        revision(&original, 3, 2, None, 2, None)?,
    )?;
    feed.heartbeat(epoch(1)?, time(2)?, None)?;
    assert!(matches!(
        policy.check(time(2)?, &[input(&feed, 1, 2)?]),
        Err(ContinuityError::Superseded)
    ));
    assert!(matches!(
        policy.check(time(2)?, &[input(&feed, 1, 3)?]),
        Err(ContinuityError::Superseded)
    ));
    Ok(())
}

fn option_record() -> TestValue<CanonicalObservation> {
    let utc = |ms| UtcInstant::from_unix_millis(ms);
    let schedule = OptionSchedule::new(
        utc(0)?,
        utc(100)?,
        utc(110)?,
        utc(110)?,
        utc(120)?,
        OptionSchedule::timezone_rules().1,
    )?;
    let spec = OptionSpecification::new(
        InstrumentId::new(9)?,
        Underlying::Index(InstrumentId::new(8)?),
        OptionTerms::new(
            OptionKind::Call,
            OptionPoints::new(100, 0)?,
            OptionPoints::new(5, 2)?,
            UsdPerPoint::new(100, 0)?,
            ExerciseStyle::European,
            SettlementStyle::Cash,
        )?,
        schedule,
        EffectiveInterval::new(utc(0)?, utc(120)?)?,
    )?;
    let version = ReferenceVersion::new(
        ReferenceId::new(20)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        utc(0)?,
        [7; 32],
        spec,
    );
    let bid = QuoteSide::new(OptionPoints::new(0, 0)?, None, time(0)?);
    let ask = QuoteSide::new(
        OptionPoints::new(5, 2)?,
        Some(DisplayedContracts::new(0)),
        time(10)?,
    );
    record(
        None,
        1,
        9,
        10,
        10,
        ObservationValue::OptionQuote(OptionQuoteSnapshot::new(&version, bid, ask)?),
    )
}

#[test]
fn quote_side_age_and_skew_are_not_hidden_by_a_fresh_snapshot_or_heartbeat() -> TestResult {
    let mut feed = tracker(
        1,
        EvidenceKind::OptionSnapshot,
        ContinuityMode::Snapshots,
        8,
        3,
    )?;
    feed.ingest(epoch(1)?, time(10)?, option_record()?)?;
    feed.heartbeat(epoch(1)?, time(10)?, None)?;
    let requirements = [required(
        1,
        9,
        EvidenceKind::OptionSnapshot,
        ContinuityRequirement::Snapshots,
    )?];
    assert!(matches!(
        plan(&requirements, 5, 20)?.check(time(10)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Stale)
    ));
    assert!(matches!(
        plan(&requirements, 20, 5)?.check(time(10)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Skew)
    ));
    plan(&requirements, 10, 10)?.check(time(10)?, &[input(&feed, 1, 1)?])?;
    Ok(())
}

#[test]
fn completed_bar_age_uses_bucket_end_and_missing_bounds_or_requirements_are_rejected() -> TestResult
{
    let kind = EvidenceKind::RawBar(BarCoverage::Intraday);
    let mut feed = tracker(1, kind, ContinuityMode::Snapshots, 8, 3)?;
    feed.ingest(epoch(1)?, time(10)?, bar(1, 0, 10)?)?;
    feed.heartbeat(epoch(1)?, time(10)?, None)?;
    let req = required(1, 9, kind, ContinuityRequirement::Snapshots)?;
    plan(&[req], 1, 0)?.check(time(10)?, &[input(&feed, 1, 1)?])?;
    assert!(matches!(
        plan(&[req], 1, 0)?.check(time(9)?, &[input(&feed, 1, 1)?]),
        Err(ContinuityError::Clock)
    ));
    for items in [vec![], vec![req, req], vec![req; 33]] {
        assert!(matches!(
            ReadinessPolicy::new(rules(2)?, Duration::from_millis(1), Duration::ZERO, &items),
            Err(ContinuityError::Policy)
        ));
    }
    assert!(matches!(
        ReadinessPolicy::new(rules(2)?, Duration::ZERO, Duration::ZERO, &[req]),
        Err(ContinuityError::Policy)
    ));
    portable::<ContinuityTracker>();
    portable::<ReadyEvidence<'static>>();
    Ok(())
}
