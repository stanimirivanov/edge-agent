use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_replay::*;
use edgeagent_simulation_domain::ArtifactDigest;
use std::time::Duration;

#[path = "../../market-data-application/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;
use support::{Fixture, Result, at, ready};
#[path = "support/selection.rs"]
#[allow(dead_code)]
mod selection;

fn now() -> Result<UtcInstant> {
    at(3 * support::DAY)
}
fn fixture() -> Result<Fixture> {
    let mut f = Fixture::new()?;
    f.request.range = DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 3)?)?;
    f.request.admitted_at = now()?;
    f.request.deadline = at(now()?.unix_millis() + 300_000)?;
    Ok(f)
}

fn source(seed: u8) -> Result<SyntheticDailySource> {
    build(seed, |_, _| {})
}
fn build(
    seed: u8,
    change: impl FnOnce(&mut AcquisitionRequest, &mut Vec<Vec<u8>>),
) -> Result<SyntheticDailySource> {
    build_revision(seed, change, 1)
}
fn build_revision(
    seed: u8,
    change: impl FnOnce(&mut AcquisitionRequest, &mut Vec<Vec<u8>>),
    revision: u64,
) -> Result<SyntheticDailySource> {
    let mut f = fixture()?;
    let mut trackers = Vec::new();
    let mut required = Vec::new();
    let mut retained = Vec::new();
    let rules = SemanticReference::new(ReferenceId::new(10)?, ReferenceRevision::new(1)?);
    let clock = PreciseInstant::from_millis(now()?);
    for index in 1..=2 {
        let feed = FeedId::new(index)?;
        let epoch = FeedEpoch::new(1)?;
        let session = SessionDate::new(1970, 1, index as u32)?;
        let start = PreciseInstant::from_millis(at((index as i64 - 1) * support::DAY)?);
        let end = PreciseInstant::from_millis(at(index as i64 * support::DAY)?);
        let observation = CanonicalObservation::new(
            ObservationContext::new(
                f.request.scope.instrument,
                f.review.capability,
                TransformationRevision::new(1)?,
                session,
                &[],
            )?,
            ObservationVersion::new(
                ObservationId::new(index)?,
                ObservationRevision::new(1)?,
                None,
            )?,
            EvidenceTimes::new(start, None, None, clock)?,
            None,
            Some(ObservationValue::Bar(RawBar::new(
                EvidenceInterval::new(start, end)?,
                BarCoverage::ProviderDay,
                [UsdPrice::new(123456789, 4)?; 4],
                ShareVolume::new(0, 0)?,
            )?)),
        )?;
        retained.push(synthetic_daily_bytes(&observation)?);
        let mut tracker = ContinuityTracker::new(
            FeedScope::new(
                feed,
                f.review.capability,
                TransformationRevision::new(1)?,
                session,
            ),
            epoch,
            ContinuityMode::Snapshots,
            ContinuityPolicy::new(
                rules,
                8,
                2,
                Duration::from_secs(10),
                Duration::from_secs(10),
            )?,
            clock,
        )?;
        tracker.ingest(epoch, clock, observation)?;
        tracker.heartbeat(epoch, clock, None)?;
        required.push(EvidenceRequirement::new(
            feed,
            f.request.scope.instrument,
            f.request.contract,
            ContinuityRequirement::Snapshots,
        ));
        trackers.push(tracker);
    }
    let policy = ReadinessPolicy::new(
        rules,
        Duration::from_secs(3 * 86400),
        Duration::from_secs(86400),
        &required,
    )?;
    let inputs = trackers
        .iter()
        .zip(1..=2)
        .map(|(tracker, id)| {
            Ok(EvidenceInput::new(
                tracker,
                FeedEpoch::new(1)?,
                ObservationId::new(id)?,
                ObservationRevision::new(1)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let checked = policy.check(clock, &inputs)?;
    change(&mut f.request, &mut retained);
    let mut selection =
        selection::SelectionFixture::new(&f.request, &[f.request.scope.instrument])?;
    let value = selection
        .selections
        .at(now()?)
        .next()
        .ok_or("selection")?
        .value()
        .clone();
    selection.selections = ReferenceHistory::new(vec![selection::version(
        1,
        revision,
        at(0)?,
        f.request.scope.source,
        value,
    )?])?;
    let plan = selection.plan()?;
    let selected = plan.prepare(&f.request)?;
    Ok(SyntheticDailySource::new(
        OfflineMode,
        &selected,
        &checked,
        &retained,
        ArtifactDigest::of(b"invented-producer-v1"),
        ArtifactDigest::of(b"invented-rights-reference-policy-selection-v1"),
        [seed; 32],
    )?)
}

#[test]
fn offline_mode_is_default_and_live_or_malformed_flags_fail_closed() {
    for value in [None, Some("1"), Some("true")] {
        assert_eq!(OfflineMode::from_mock_flag(value), Ok(OfflineMode));
    }
    for value in [
        "0",
        "false",
        "",
        "TRUE",
        " 1",
        "live",
        "https://invalid.invalid",
    ] {
        assert_eq!(
            OfflineMode::from_mock_flag(Some(value)),
            Err(OfflineError::Configuration)
        );
    }
    assert_eq!(
        format!("{}", OfflineError::Configuration),
        "offline source rejected: Configuration"
    );
}

#[test]
fn daily_source_returns_meaningful_repeatable_canonical_batches() -> Result {
    let source = source(7)?;
    let f = fixture()?;
    let cancel = Cancellation::default();
    let admitted = f.catalogs().admit(&f.request, now()?, &cancel)?;
    let selection = selection::SelectionFixture::new(&f.request, &[f.request.scope.instrument])?;
    let plan = selection.plan()?;
    let selected = SelectedDailyProvider::new(&plan, &source);
    let first = ready(selected.fetch(&admitted, &cancel))??;
    assert_eq!(first.observations.len(), 2);
    assert_eq!(first, ready(source.fetch(&admitted, &cancel))??);
    validate_batch(&admitted, f.catalogs(), now()?, &cancel, &first)?;
    assert_eq!(
        first
            .observations
            .first()
            .and_then(|o| o.value())
            .and_then(|v| match v {
                ObservationValue::Bar(b) => Some(b.values().0[0].decimal_parts()),
                _ => None,
            }),
        Some((123456789, 4))
    );
    // Fetch is repeatable and does not advance a consumer's independent committed replay cursor.
    assert_eq!(format!("{source:?}"), "SyntheticDailySource(..)");
    Ok(())
}

#[test]
fn replay_restores_every_prefix_without_repeating_ids_or_random_draws() -> Result {
    let fixture = source(7)?;
    let cancel = Cancellation::default();
    let mut replay = fixture.replay();
    let mut checkpoints = vec![replay.checkpoint()];
    let mut expected = Vec::new();
    while let Some(step) = replay.next(&cancel)? {
        expected.push((step.observation.clone(), step.id, step.random));
        assert_eq!(replay.now(), PreciseInstant::from_millis(now()?));
        checkpoints.push(replay.checkpoint());
    }
    assert_eq!(expected.len(), 2);
    // The source itself binds selection content/revisions; callers cannot forget to hash them.
    let revised = build_revision(7, |_, _| {}, 2)?;
    assert!(
        revised
            .restore(checkpoints.first().ok_or("checkpoint")?)
            .is_err()
    );
    for (prefix, checkpoint) in checkpoints.iter().enumerate() {
        let rebuilt = source(7)?;
        let mut resumed = rebuilt.restore(checkpoint)?;
        let mut actual = Vec::new();
        while let Some(step) = resumed.next(&cancel)? {
            actual.push((step.observation.clone(), step.id, step.random));
        }
        assert_eq!(actual, expected.get(prefix..).ok_or("prefix")?);
        assert_eq!(resumed.checkpoint(), replay.checkpoint());
    }
    assert!(source(8)?.restore(&checkpoints[0]).is_err());
    let changed_id = AcquisitionId::new(2)?;
    let changed = build(7, |request, _| request.id = changed_id)?;
    assert!(changed.restore(&checkpoints[0]).is_err());
    let mut corrupt = checkpoints[1].clone();
    corrupt[0] ^= 1;
    assert!(fixture.restore(&corrupt).is_err());
    assert!(fixture.restore(&corrupt[..20]).is_err());
    Ok(())
}

#[test]
fn fixture_bytes_scope_bounds_and_provenance_are_checked_before_replay() -> Result {
    assert!(
        build(7, |_, bytes| {
            bytes[0][0] ^= 1;
        })
        .is_err()
    );
    assert!(
        build(7, |_, bytes| {
            bytes.swap(0, 1);
        })
        .is_err()
    );
    assert!(
        build(7, |_, bytes| {
            bytes.clear();
        })
        .is_err()
    );
    assert!(
        build(7, |_, bytes| {
            bytes[0] = vec![0; 4_194_305];
        })
        .is_err()
    );
    let foreign_source = SourceId::new(2)?;
    assert!(
        build(7, |r, _| {
            r.scope.source = foreign_source;
        })
        .is_err()
    );
    assert!(
        build(7, |r, _| {
            r.knowledge = KnowledgeMode::RetainedAt(r.admitted_at);
        })
        .is_err()
    );
    let f = Fixture::new()?;
    let mut observation = f.batch(now()?)?.observations.remove(0);
    let original = synthetic_daily_bytes(&observation)?;
    observation = CanonicalObservation::new(
        observation.context().clone(),
        observation.version(),
        EvidenceTimes::new(
            observation.times().event_at(),
            None,
            None,
            PreciseInstant::from_millis(at(support::NOW + 1)?),
        )?,
        None,
        observation.value(),
    )?;
    assert_ne!(original, synthetic_daily_bytes(&observation)?);
    Ok(())
}

#[test]
fn cancellation_and_foreign_admission_leave_the_checkpoint_unchanged() -> Result {
    let source = source(7)?;
    let mut f = fixture()?;
    let cancel = Cancellation::default();
    let mut replay = source.replay();
    let before = replay.checkpoint();
    cancel.cancel();
    assert!(matches!(replay.next(&cancel), Err(OfflineError::Cancelled)));
    assert_eq!(replay.checkpoint(), before);
    let admitted = f
        .catalogs()
        .admit(&f.request, now()?, &Cancellation::default())?;
    assert_eq!(
        ready(source.fetch(&admitted, &cancel))?,
        Err(ProviderFailure::Cancelled)
    );
    f.request.scope.account = ProviderAccountId::new(2)?;
    f.right.scope = f.request.scope;
    f.review.scope = f.request.scope;
    f.rights = ReferenceHistory::new(vec![support::version(1, 1, at(0)?, f.right)?])?;
    f.providers = ReferenceHistory::new(vec![support::version(1, 1, at(0)?, f.review)?])?;
    let admitted = f
        .catalogs()
        .admit(&f.request, now()?, &Cancellation::default())?;
    assert_eq!(
        ready(source.fetch(&admitted, &Cancellation::default()))?,
        Err(ProviderFailure::Unsupported)
    );
    Ok(())
}

#[test]
fn ambient_credentials_cannot_activate_live_access() -> Result {
    if std::env::var_os("EDGEAGENT_OFFLINE_CHILD").is_some() {
        assert!(std::env::var_os("FINNHUB_API_KEY").is_some());
        daily_source_returns_meaningful_repeatable_canonical_batches()?;
        replay_restores_every_prefix_without_repeating_ids_or_random_draws()?;
        return Ok(());
    }
    let status = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "ambient_credentials_cannot_activate_live_access"])
        .env("EDGEAGENT_OFFLINE_CHILD", "1")
        .env("FINNHUB_API_KEY", "offline")
        .env("APCA_API_KEY_ID", "offline")
        .env("APCA_API_SECRET_KEY", "offline")
        .env("USE_MOCK", "0")
        .status()?;
    assert!(status.success());
    Ok(())
}
