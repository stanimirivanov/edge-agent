use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{
    ArtifactDigest, FrozenInput, Run, RunManifest, orders::*, risk::*,
};
use std::time::Duration;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub fn at(second: i64) -> TestResult<PreciseInstant> {
    Ok(PreciseInstant::from_unix_parts(second, 0)?)
}
pub fn digest() -> ArtifactDigest {
    ArtifactDigest::of(b"synthetic-observation")
}
pub fn model_identity() -> TestResult<(SemanticReference, ArtifactDigest)> {
    Ok((
        SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
        ArtifactDigest::of(b"model-config"),
    ))
}
pub fn price(value: i128) -> TestResult<OrderPrice> {
    Ok(OrderPrice::PerShare(UsdPrice::new(value, 0)?))
}
pub fn quantity(value: u64) -> TestResult<OrderQuantity> {
    Ok(OrderQuantity::shares(value)?)
}

pub struct Evidence {
    tracker: ContinuityTracker,
    policy: ReadinessPolicy,
}
impl Evidence {
    pub fn new(
        instrument: u128,
        value: ObservationValue,
        kind: EvidenceKind,
        market: MarketScope,
    ) -> TestResult<Self> {
        let rules = model_identity()?.0;
        let contract = EvidenceContract::new(kind, market, rules, TimestampPrecision::Nanoseconds)?;
        let provider = ProviderCapability::new(
            SourceId::new(1)?,
            contract,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        );
        let session = SessionDate::new(1970, 1, 1)?;
        let transformation = TransformationRevision::new(1)?;
        let instrument = InstrumentId::new(instrument)?;
        let observation = CanonicalObservation::new(
            ObservationContext::new(instrument, provider, transformation, session, &[])?,
            ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
            EvidenceTimes::new(at(0)?, None, None, at(0)?)?,
            None,
            Some(value),
        )?;
        let mut tracker = ContinuityTracker::new(
            FeedScope::new(FeedId::new(1)?, provider, transformation, session),
            FeedEpoch::new(1)?,
            ContinuityMode::Snapshots,
            ContinuityPolicy::new(
                rules,
                8,
                2,
                Duration::from_secs(20),
                Duration::from_secs(20),
            )?,
            at(0)?,
        )?;
        tracker.ingest(FeedEpoch::new(1)?, at(0)?, observation)?;
        tracker.heartbeat(FeedEpoch::new(1)?, at(1)?, None)?;
        let policy = ReadinessPolicy::new(
            rules,
            Duration::from_secs(20),
            Duration::ZERO,
            &[EvidenceRequirement::new(
                FeedId::new(1)?,
                instrument,
                contract,
                ContinuityRequirement::Snapshots,
            )],
        )?;
        Ok(Self { tracker, policy })
    }
    pub fn trade() -> TestResult<Self> {
        Self::new(
            1,
            ObservationValue::Trade(ShareTrade::new(
                UsdPrice::new(100, 0)?,
                ShareVolume::new(10, 0)?,
            )?),
            EvidenceKind::ShareTrade,
            MarketScope::Consolidated,
        )
    }
    pub fn ready(&self) -> TestResult<ReadyEvidence<'_>> {
        self.ready_at(1)
    }
    pub fn ready_at(&self, second: i64) -> TestResult<ReadyEvidence<'_>> {
        Ok(self.policy.check(
            at(second)?,
            &[EvidenceInput::new(
                &self.tracker,
                FeedEpoch::new(1)?,
                ObservationId::new(1)?,
                ObservationRevision::new(1)?,
            )],
        )?)
    }
    pub fn manifest(&self) -> TestResult<RunManifest> {
        Ok(RunManifest::new(
            at(0)?,
            ArtifactDigest::of(b"code"),
            ArtifactDigest::of(b"run-config"),
            [0; 32],
            &FrozenInput::from_ready(&self.ready()?, &[digest()])?,
        )?)
    }
}
pub fn definition(run: &mut Run<'_>, side: OrderSide, count: u64) -> TestResult<OrderDefinition> {
    Ok(OrderDefinition::new(
        DryRunMode::parse(Some("dry_run"))?,
        OrderScope::new(
            run.manifest_digest(),
            RiskAccountId::new(1)?,
            RiskStrategyId::new(1)?,
            OrderId::new(run.next_id()?),
        ),
        OrderInstrument::equity(InstrumentId::new(1)?),
        side,
        OrderKind::Limit(price(100)?),
        quantity(count)?,
        EvidenceInterval::new(at(1)?, at(10)?)?,
    )?)
}
pub fn event(
    run: &mut Run<'_>,
    order: &Order,
    revision: u64,
    second: i64,
    kind: OrderEventKind,
) -> TestResult<OrderEvent> {
    Ok(OrderEvent::new(
        OrderEventId::new(run.next_id()?),
        order.definition().scope().identity().3,
        revision,
        at(second)?,
        kind,
    )?)
}
pub fn fill(count: u64, value: i128) -> TestResult<OrderEventKind> {
    Ok(OrderEventKind::Filled(FillFact::new(
        ProposedFill::new(quantity(count)?, price(value)?),
        model_identity()?,
        digest(),
    )))
}
pub fn apply(
    run: &mut Run<'_>,
    order: &mut Order,
    second: i64,
    kind: OrderEventKind,
) -> TestResult<OrderEvent> {
    let event = event(run, order, order.state().1 + 1, second, kind)?;
    assert_eq!(order.apply(event)?, ApplyOutcome::Applied);
    Ok(event)
}
pub fn unchanged(order: &mut Order, event: OrderEvent, expected: OrderError) {
    let state = order.state();
    let quantities = order.quantities();
    let journal = order.events().to_vec();
    assert_eq!(order.apply(event), Err(expected));
    assert_eq!(order.state(), state);
    assert_eq!(order.quantities(), quantities);
    assert_eq!(order.events(), journal);
}
pub fn option(
    exercise: ExerciseStyle,
    settlement: SettlementStyle,
) -> TestResult<ReferenceVersion<OptionSpecification>> {
    let utc = |second: i64| UtcInstant::from_unix_millis(second * 1000);
    let spec = OptionSpecification::new(
        InstrumentId::new(10)?,
        Underlying::Index(InstrumentId::new(2)?),
        OptionTerms::new(
            OptionKind::Call,
            OptionPoints::new(100, 0)?,
            OptionPoints::new(5, 2)?,
            UsdPerPoint::new(100, 0)?,
            exercise,
            settlement,
        )?,
        OptionSchedule::new(
            utc(0)?,
            utc(10)?,
            utc(11)?,
            utc(11)?,
            utc(12)?,
            OptionSchedule::timezone_rules().1,
        )?,
        EffectiveInterval::new(utc(0)?, utc(20)?)?,
    )?;
    Ok(ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        utc(0)?,
        [7; 32],
        spec,
    ))
}
