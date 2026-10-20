use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{ArtifactDigest, FrozenInput, Run, RunError, RunManifest};
use std::time::Duration;

fn portable<T: Send + Sync>() {}

#[test]
fn readiness_is_the_availability_boundary_not_event_time() -> Result<(), Box<dyn std::error::Error>>
{
    let start = PreciseInstant::from_unix_parts(0, 0)?;
    let available = PreciseInstant::from_unix_parts(1, 0)?;
    let rules = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
    let feed = FeedId::new(1)?;
    let epoch = FeedEpoch::new(1)?;
    let instrument = InstrumentId::new(1)?;
    let contract = EvidenceContract::new(
        EvidenceKind::IndexLevel,
        MarketScope::IndexPublisher,
        rules,
        TimestampPrecision::Nanoseconds,
    )?;
    let provider = ProviderCapability::new(
        SourceId::new(1)?,
        contract,
        DeliveryCapability::Snapshots,
        KnowledgeCapability::CaptureOnly,
    );
    let session = SessionDate::new(1970, 1, 1)?;
    let transformation = TransformationRevision::new(1)?;
    let id = ObservationId::new(1)?;
    let revision = ObservationRevision::new(1)?;
    let observation = CanonicalObservation::new(
        ObservationContext::new(instrument, provider, transformation, session, &[])?,
        ObservationVersion::new(id, revision, None)?,
        EvidenceTimes::new(start, None, None, start)?,
        None,
        Some(ObservationValue::IndexLevel(IndexPoints::new(100, 0)?)),
    )?;
    let mut tracker = ContinuityTracker::new(
        FeedScope::new(feed, provider, transformation, session),
        epoch,
        ContinuityMode::Snapshots,
        ContinuityPolicy::new(
            rules,
            8,
            2,
            Duration::from_secs(10),
            Duration::from_secs(10),
        )?,
        start,
    )?;
    tracker.ingest(epoch, start, observation)?;
    tracker.heartbeat(epoch, available, None)?;
    let policy = ReadinessPolicy::new(
        rules,
        Duration::from_secs(10),
        Duration::ZERO,
        &[EvidenceRequirement::new(
            feed,
            instrument,
            contract,
            ContinuityRequirement::Snapshots,
        )],
    )?;
    let candidate = EvidenceInput::new(&tracker, epoch, id, revision);
    let ready = policy.check(available, &[candidate])?;
    assert_eq!(FrozenInput::from_ready(&ready, &[]), Err(RunError::Input));
    let digest = ArtifactDigest::of(b"synthetic-retained-record-v1");
    let frozen = FrozenInput::from_ready(&ready, &[digest])?;
    assert_eq!(frozen[0].available_at(), available);
    assert_eq!(frozen[0].artifact(), digest);
    let manifest = RunManifest::new(
        start,
        ArtifactDigest::of(b"code"),
        ArtifactDigest::of(b"config"),
        [0; 32],
        &frozen,
    )?;
    let mut run = Run::new(&manifest);
    assert_eq!(run.now(), start);
    assert_eq!(
        run.next_input().map(FrozenInput::available_at),
        Some(available)
    );
    assert_eq!(run.now(), available);
    assert!(run.next_input().is_none());
    portable::<RunManifest>();
    portable::<Run<'_>>();
    Ok(())
}
