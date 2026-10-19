#[path = "continuity/support.rs"]
pub mod support;
use edgeagent_market_data_domain::*;
use std::time::Duration;
use support::*;

#[test]
fn gaps_repair_in_order_but_overlap_and_heartbeats_never_release_the_barrier() -> TestResult {
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        8,
        3,
    )?;
    feed.heartbeat(epoch(1)?, time(0)?, None)?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(0)?, index(Some(2), 3, 0, 0)?)?,
        IngestOutcome::Buffered
    );
    assert_eq!(feed.missing_range(), Some((0, 2)));
    assert_eq!(feed.status(), (ContinuityState::Recovering, 1, 0));
    let ticket = feed.begin_recovery(epoch(1)?, time(0)?)?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(1)?, index(Some(0), 1, 0, 0)?)?,
        IngestOutcome::Accepted { released: 1 }
    );
    assert_eq!(
        feed.ingest(epoch(1)?, time(2)?, index(Some(0), 1, 0, 2)?)?,
        IngestOutcome::Duplicate
    );
    assert_eq!(
        feed.ingest(epoch(1)?, time(3)?, index(Some(1), 2, 0, 0)?)?,
        IngestOutcome::Accepted { released: 2 }
    );
    assert_eq!(feed.missing_range(), None);
    feed.heartbeat(epoch(1)?, time(3)?, Some(2))?;
    assert_eq!(feed.status().0, ContinuityState::Recovering);
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
        policy.check(time(3)?, &[input(&feed, 3, 1)?]),
        Err(ContinuityError::Recovering)
    ));
    feed.finish_recovery(ticket, time(3)?)?;
    let ready = policy.check(time(3)?, &[input(&feed, 3, 1)?])?;
    assert_eq!(ready.observations().len(), 1);
    assert_eq!(
        ready
            .observations()
            .first()
            .map(|r| r.times().received_at()),
        Some(time(0)?)
    );
    assert_eq!(feed.status(), (ContinuityState::Tracking, 3, 1));
    Ok(())
}

#[test]
fn conflicting_sequence_or_revision_halts_without_silent_replacement() -> TestResult {
    for conflict in [
        index(Some(0), 2, 0, 0)?,
        record(
            Some(0),
            1,
            9,
            0,
            0,
            ObservationValue::IndexLevel(IndexPoints::new(2, 0)?),
        )?,
        index(Some(1), 1, 0, 0)?,
    ] {
        let mut feed = tracker(
            1,
            EvidenceKind::IndexLevel,
            ContinuityMode::Contiguous { next: 0 },
            4,
            3,
        )?;
        feed.ingest(epoch(1)?, time(0)?, index(Some(0), 1, 0, 0)?)?;
        assert_eq!(
            feed.ingest(epoch(1)?, time(0)?, conflict),
            Err(ContinuityError::Conflict)
        );
        assert_eq!(feed.status(), (ContinuityState::Halted, 1, 0));
        assert_eq!(
            feed.begin_recovery(epoch(1)?, time(0)?),
            Err(ContinuityError::Halted)
        );
    }
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 4 },
        4,
        3,
    )?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(0)?, index(Some(3), 1, 0, 0)?),
        Err(ContinuityError::Conflict)
    );
    Ok(())
}

#[test]
fn corrections_wait_for_their_current_predecessor_and_missing_history_halts() -> TestResult {
    let original = index(Some(0), 1, 0, 0)?;
    let corrected = revision(
        &original,
        2,
        1,
        Some(1),
        1,
        Some(ObservationValue::IndexLevel(IndexPoints::new(2, 0)?)),
    )?;
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        8,
        3,
    )?;
    feed.ingest(epoch(1)?, time(1)?, corrected.clone())?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(1)?, original.clone())?,
        IngestOutcome::Accepted { released: 2 }
    );
    let stale = revision(&original, 3, 1, Some(2), 2, original.value())?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(2)?, stale),
        Err(ContinuityError::Predecessor)
    );
    let mut missing = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 1 },
        8,
        3,
    )?;
    assert_eq!(
        missing.ingest(epoch(1)?, time(1)?, corrected),
        Err(ContinuityError::Predecessor)
    );
    assert_eq!(missing.status().0, ContinuityState::Halted);
    Ok(())
}

#[test]
fn current_distinct_bars_cannot_overlap_but_adjacent_or_cancelled_buckets_can() -> TestResult {
    let kind = EvidenceKind::RawBar(BarCoverage::Intraday);
    let mut feed = tracker(1, kind, ContinuityMode::Snapshots, 8, 3)?;
    let first = bar(1, 0, 5)?;
    feed.ingest(epoch(1)?, time(5)?, first.clone())?;
    feed.ingest(epoch(1)?, time(10)?, bar(2, 5, 10)?)?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(10)?, bar(3, 4, 9)?),
        Err(ContinuityError::Overlap)
    );
    let mut feed = tracker(1, kind, ContinuityMode::Snapshots, 8, 3)?;
    feed.ingest(epoch(1)?, time(5)?, first.clone())?;
    feed.ingest(epoch(1)?, time(6)?, revision(&first, 2, 1, None, 6, None)?)?;
    feed.ingest(epoch(1)?, time(6)?, bar(3, 0, 5)?)?;
    assert_eq!(feed.status().1, 3);
    Ok(())
}

#[test]
fn heartbeat_gaps_latch_and_watermarks_reveal_missing_data_without_refreshing_prices() -> TestResult
{
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        8,
        3,
    )?;
    feed.heartbeat(epoch(1)?, time(0)?, None)?;
    feed.ingest(epoch(1)?, time(0)?, index(Some(0), 1, 0, 0)?)?;
    feed.heartbeat(epoch(1)?, time(11)?, Some(0))?;
    assert_eq!(feed.status().0, ContinuityState::Recovering);
    let attempt = feed.begin_recovery(epoch(1)?, time(11)?)?;
    feed.finish_recovery(attempt, time(11)?)?;
    feed.heartbeat(epoch(1)?, time(12)?, Some(2))?;
    assert_eq!(feed.missing_range(), Some((1, 2)));
    let ticket = feed.begin_recovery(epoch(1)?, time(12)?)?;
    assert_eq!(
        feed.finish_recovery(ticket, time(12)?),
        Err(ContinuityError::Gap)
    );
    assert_eq!(
        feed.finish_recovery(ticket, time(12)?),
        Err(ContinuityError::StaleRecovery)
    );
    assert_eq!(
        feed.heartbeat(epoch(1)?, time(12)?, Some(1)),
        Err(ContinuityError::Watermark)
    );
    assert_eq!(
        feed.heartbeat(epoch(1)?, time(11)?, Some(2)),
        Err(ContinuityError::Clock)
    );
    Ok(())
}

#[test]
fn capacity_and_sequence_overflow_stop_without_eviction_wrap_or_unbounded_ranges() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 1, 3)?;
    feed.ingest(epoch(1)?, time(0)?, index(None, 1, 0, 0)?)?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(1)?, index(None, 1, 0, 1)?)?,
        IngestOutcome::Duplicate
    );
    assert_eq!(
        feed.ingest(epoch(1)?, time(1)?, index(None, 2, 0, 0)?),
        Err(ContinuityError::Capacity)
    );
    assert_eq!(feed.status(), (ContinuityState::Exhausted, 1, 0));
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: u64::MAX },
        2,
        3,
    )?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(0)?, index(Some(u64::MAX), 1, 0, 0)?),
        Err(ContinuityError::SequenceOverflow)
    );
    assert_eq!(feed.status().0, ContinuityState::Exhausted);
    let mut feed = tracker(
        1,
        EvidenceKind::IndexLevel,
        ContinuityMode::Contiguous { next: 0 },
        2,
        3,
    )?;
    feed.heartbeat(epoch(1)?, time(0)?, Some(u64::MAX))?;
    assert_eq!(feed.missing_range(), Some((0, 1)));
    Ok(())
}

#[test]
fn reset_and_stale_work_never_refund_budget_or_poison_a_newer_fence() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 4, 2)?;
    feed.reset(epoch(2)?, ContinuityMode::Snapshots, time(0)?)?;
    let old = feed.begin_recovery(epoch(2)?, time(0)?)?;
    feed.abandon_recovery(old)?;
    let current = feed.begin_recovery(epoch(2)?, time(0)?)?;
    assert_eq!(
        feed.ingest_recovered(old, time(1000)?, index(None, 1, 0, 0)?),
        Err(ContinuityError::StaleRecovery)
    );
    assert_eq!(
        feed.finish_recovery(old, time(1000)?),
        Err(ContinuityError::StaleRecovery)
    );
    assert_eq!(
        feed.heartbeat(epoch(1)?, time(1000)?, None),
        Err(ContinuityError::Epoch)
    );
    assert_eq!(
        feed.ingest(epoch(1)?, time(1000)?, index(None, 1, 0, 0)?),
        Err(ContinuityError::Epoch)
    );
    feed.heartbeat(epoch(2)?, time(0)?, None)?;
    feed.finish_recovery(current, time(0)?)?;
    feed.reset(epoch(3)?, ContinuityMode::Snapshots, time(0)?)?;
    assert_eq!(feed.status(), (ContinuityState::Exhausted, 0, 2));
    assert_eq!(
        feed.begin_recovery(epoch(3)?, time(0)?),
        Err(ContinuityError::Exhausted)
    );
    assert_eq!(
        feed.reset(epoch(3)?, ContinuityMode::Snapshots, time(0)?),
        Err(ContinuityError::Epoch)
    );
    assert_eq!(
        feed.finish_recovery(current, time(0)?),
        Err(ContinuityError::StaleRecovery)
    );
    Ok(())
}

#[test]
fn recovery_output_is_fenced_by_feed_and_expiry_before_it_can_mutate_state() -> TestResult {
    let mut first = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 4, 3)?;
    let mut second = tracker(2, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 4, 3)?;
    first.reset(epoch(2)?, ContinuityMode::Snapshots, time(0)?)?;
    second.reset(epoch(2)?, ContinuityMode::Snapshots, time(0)?)?;
    let foreign = first.begin_recovery(epoch(2)?, time(0)?)?;
    let own = second.begin_recovery(epoch(2)?, time(0)?)?;
    assert_eq!(
        second.ingest_recovered(foreign, time(1000)?, index(None, 1, 0, 0)?),
        Err(ContinuityError::StaleRecovery)
    );
    assert_eq!(
        second.finish_recovery(foreign, time(1000)?),
        Err(ContinuityError::StaleRecovery)
    );
    second.ingest_recovered(own, time(0)?, index(None, 1, 0, 0)?)?;
    second.heartbeat(epoch(2)?, time(0)?, None)?;
    second.finish_recovery(own, time(0)?)?;
    assert_eq!(second.status(), (ContinuityState::Tracking, 1, 1));
    assert_eq!(
        first.ingest_recovered(foreign, time(21)?, index(None, 1, 0, 0)?),
        Err(ContinuityError::StaleRecovery)
    );
    assert_eq!(first.status().1, 0);
    Ok(())
}

#[test]
fn foreign_scope_and_future_receipts_do_not_advance_the_healthy_clock() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 4, 3)?;
    let original = index(None, 1, 0, 0)?;
    let ctx = ObservationContext::new(
        InstrumentId::new(9)?,
        ProviderCapability::new(
            SourceId::new(2)?,
            contract(EvidenceKind::IndexLevel)?,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        ),
        TransformationRevision::new(1)?,
        SessionDate::new(1970, 1, 1)?,
        &[],
    )?;
    let foreign = CanonicalObservation::new(
        ctx,
        original.version(),
        original.times(),
        None,
        original.value(),
    )?;
    assert_eq!(
        feed.ingest(epoch(1)?, time(1000)?, foreign),
        Err(ContinuityError::Scope)
    );
    feed.ingest(epoch(1)?, time(0)?, original)?;
    feed.heartbeat(epoch(1)?, time(0)?, None)?;
    assert_eq!(feed.status(), (ContinuityState::Tracking, 1, 0));
    Ok(())
}

#[test]
fn recovery_counts_duplicate_row_work_and_expires_without_automatic_retries() -> TestResult {
    let mut feed = tracker(1, EvidenceKind::IndexLevel, ContinuityMode::Snapshots, 2, 4)?;
    feed.reset(epoch(2)?, ContinuityMode::Snapshots, time(0)?)?;
    let first = feed.begin_recovery(epoch(2)?, time(0)?)?;
    assert_eq!(
        feed.begin_recovery(epoch(2)?, time(0)?),
        Err(ContinuityError::RecoveryInFlight)
    );
    let row = index(None, 1, 0, 0)?;
    feed.ingest(epoch(2)?, time(0)?, row.clone())?;
    feed.ingest(epoch(2)?, time(0)?, row.clone())?;
    assert_eq!(
        feed.ingest(epoch(2)?, time(0)?, row),
        Err(ContinuityError::RecoveryWork)
    );
    assert_eq!(
        feed.finish_recovery(first, time(0)?),
        Err(ContinuityError::StaleRecovery)
    );
    let expired = feed.begin_recovery(epoch(2)?, time(0)?)?;
    feed.heartbeat(epoch(2)?, time(21)?, None)?;
    assert_eq!(
        feed.finish_recovery(expired, time(21)?),
        Err(ContinuityError::StaleRecovery)
    );
    let fresh = feed.begin_recovery(epoch(2)?, time(21)?)?;
    feed.heartbeat(epoch(2)?, time(41)?, None)?;
    feed.finish_recovery(fresh, time(41)?)?;
    assert_eq!(feed.status(), (ContinuityState::Tracking, 1, 3));
    Ok(())
}

#[test]
fn explicit_policy_bounds_scope_and_deterministic_replay_are_checked() -> TestResult {
    for (capacity, attempts, heartbeat, recovery) in [
        (0, 1, 1, 1),
        (4097, 1, 1, 1),
        (1, 0, 1, 1),
        (1, 65, 1, 1),
        (1, 1, 0, 1),
        (1, 1, 1, 0),
    ] {
        assert_eq!(
            ContinuityPolicy::new(
                rules(1)?,
                capacity,
                attempts,
                Duration::from_nanos(heartbeat),
                Duration::from_nanos(recovery)
            ),
            Err(ContinuityError::Policy)
        );
    }
    assert!(
        ContinuityPolicy::new(
            rules(1)?,
            4096,
            64,
            Duration::from_nanos(1),
            Duration::from_nanos(1)
        )
        .is_ok()
    );
    assert_eq!(FeedId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(FeedEpoch::new(0), Err(ValueError::ZeroIdentity));
    let mut outcomes = Vec::new();
    for _ in 0..2 {
        let mut feed = tracker(
            1,
            EvidenceKind::IndexLevel,
            ContinuityMode::Contiguous { next: 0 },
            8,
            3,
        )?;
        let mut run = Vec::new();
        for sequence in [2, 0, 0, 1] {
            run.push(feed.ingest(
                epoch(1)?,
                time(0)?,
                index(Some(sequence), u128::from(sequence) + 1, 0, 0)?,
            )?);
        }
        feed.heartbeat(epoch(1)?, time(0)?, Some(2))?;
        let ticket = feed.begin_recovery(epoch(1)?, time(0)?)?;
        feed.finish_recovery(ticket, time(0)?)?;
        assert!(!format!("{feed:?} {ticket:?}").contains("987654321"));
        assert!(!format!("{feed:?}").contains("65530"));
        outcomes.push((run, feed.status()));
    }
    assert_eq!(outcomes.first(), outcomes.last());
    Ok(())
}
