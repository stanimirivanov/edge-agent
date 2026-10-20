use edgeagent_market_data_domain::*;
use std::time::Duration;
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;
pub(crate) type TestValue<T> = Result<T, Box<dyn std::error::Error>>;
pub(crate) fn time(ms: i64) -> TestValue<PreciseInstant> {
    Ok(PreciseInstant::from_millis(UtcInstant::from_unix_millis(
        ms,
    )?))
}
pub(crate) fn epoch(value: u64) -> TestValue<FeedEpoch> {
    Ok(FeedEpoch::new(value)?)
}
pub(crate) fn rules(value: u64) -> TestValue<SemanticReference> {
    Ok(SemanticReference::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(value)?,
    ))
}
pub(crate) fn contract(kind: EvidenceKind) -> TestValue<EvidenceContract> {
    Ok(EvidenceContract::new(
        kind,
        if kind == EvidenceKind::IndexLevel {
            MarketScope::IndexPublisher
        } else {
            MarketScope::Consolidated
        },
        rules(1)?,
        TimestampPrecision::Nanoseconds,
    )?)
}
pub(crate) fn provider(kind: EvidenceKind, sequenced: bool) -> TestValue<ProviderCapability> {
    Ok(ProviderCapability::new(
        SourceId::new(1)?,
        contract(kind)?,
        if sequenced {
            DeliveryCapability::Sequenced
        } else {
            DeliveryCapability::Snapshots
        },
        KnowledgeCapability::CaptureOnly,
    ))
}
pub(crate) fn tracker(
    feed: u128,
    kind: EvidenceKind,
    mode: ContinuityMode,
    capacity: usize,
    attempts: u16,
) -> TestValue<ContinuityTracker> {
    Ok(ContinuityTracker::new(
        FeedScope::new(
            FeedId::new(feed)?,
            provider(kind, mode != ContinuityMode::Snapshots)?,
            TransformationRevision::new(1)?,
            SessionDate::new(1970, 1, 1)?,
        ),
        epoch(1)?,
        mode,
        ContinuityPolicy::new(
            rules(1)?,
            capacity,
            attempts,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )?,
        time(0)?,
    )?)
}
pub(crate) fn record(
    sequence: Option<u64>,
    id: u128,
    instrument: u128,
    event: i64,
    received: i64,
    value: ObservationValue,
) -> TestValue<CanonicalObservation> {
    let kind = match value {
        ObservationValue::Bar(bar) => EvidenceKind::RawBar(bar.bucket().1),
        ObservationValue::Trade(_) => EvidenceKind::ShareTrade,
        ObservationValue::IndexLevel(_) => EvidenceKind::IndexLevel,
        ObservationValue::OptionQuote(_) => EvidenceKind::OptionSnapshot,
        _ => return Err("unsupported fixture shape".into()),
    };
    Ok(CanonicalObservation::new(
        ObservationContext::new(
            InstrumentId::new(instrument)?,
            provider(kind, sequence.is_some())?,
            TransformationRevision::new(1)?,
            SessionDate::new(1970, 1, 1)?,
            &[1, 65530],
        )?,
        ObservationVersion::new(ObservationId::new(id)?, ObservationRevision::new(1)?, None)?,
        EvidenceTimes::new(time(event)?, None, None, time(received)?)?,
        sequence,
        Some(value),
    )?)
}
pub(crate) fn index(
    sequence: Option<u64>,
    id: u128,
    event: i64,
    received: i64,
) -> TestValue<CanonicalObservation> {
    record(
        sequence,
        id,
        9,
        event,
        received,
        ObservationValue::IndexLevel(IndexPoints::new(987_654_321, 0)?),
    )
}
pub(crate) fn revision(
    original: &CanonicalObservation,
    revision: u64,
    previous: u64,
    sequence: Option<u64>,
    received: i64,
    value: Option<ObservationValue>,
) -> TestValue<CanonicalObservation> {
    Ok(CanonicalObservation::new(
        original.context().clone(),
        ObservationVersion::new(
            original.version().lineage().0,
            ObservationRevision::new(revision)?,
            Some(ObservationRevision::new(previous)?),
        )?,
        EvidenceTimes::new(original.times().event_at(), None, None, time(received)?)?,
        sequence,
        value,
    )?)
}
pub(crate) fn bar(id: u128, start: i64, end: i64) -> TestValue<CanonicalObservation> {
    record(
        None,
        id,
        9,
        start,
        end,
        ObservationValue::Bar(RawBar::new(
            EvidenceInterval::new(time(start)?, time(end)?)?,
            BarCoverage::Intraday,
            [UsdPrice::new(1, 0)?; 4],
            ShareVolume::new(0, 0)?,
        )?),
    )
}
pub(crate) fn input(
    tracker: &ContinuityTracker,
    id: u128,
    revision: u64,
) -> TestValue<EvidenceInput<'_>> {
    Ok(EvidenceInput::new(
        tracker,
        tracker.identity().1,
        ObservationId::new(id)?,
        ObservationRevision::new(revision)?,
    ))
}
pub(crate) fn plan(
    required: &[EvidenceRequirement],
    age: u64,
    skew: u64,
) -> TestValue<ReadinessPolicy> {
    Ok(ReadinessPolicy::new(
        rules(2)?,
        Duration::from_millis(age),
        Duration::from_millis(skew),
        required,
    )?)
}
pub(crate) fn required(
    feed: u128,
    instrument: u128,
    kind: EvidenceKind,
    continuity: ContinuityRequirement,
) -> TestValue<EvidenceRequirement> {
    Ok(EvidenceRequirement::new(
        FeedId::new(feed)?,
        InstrumentId::new(instrument)?,
        contract(kind)?,
        continuity,
    ))
}
