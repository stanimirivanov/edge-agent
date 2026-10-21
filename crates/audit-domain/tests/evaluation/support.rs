use edgeagent_audit_domain::{AccountProjection, evaluation::*, fingerprint};
use edgeagent_contracts::audit::*;
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{
    ArtifactDigest, FrozenInput, Run, RunManifest, execution::*, orders::*, risk::*, settlement::*,
};
use std::time::Duration;
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn at(s: i64) -> Result<PreciseInstant> {
    Ok(PreciseInstant::from_unix_parts(s, 0)?)
}
fn utc(s: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(s * 1000)?)
}
pub fn digest(b: &[u8]) -> ArtifactDigest {
    ArtifactDigest::of(b)
}
pub fn hex(d: ArtifactDigest) -> String {
    d.bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn rules() -> Result<SemanticReference> {
    Ok(SemanticReference::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
    ))
}
fn spec() -> Result<ReferenceVersion<OptionSpecification>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        utc(0)?,
        [7; 32],
        OptionSpecification::new(
            InstrumentId::new(10)?,
            Underlying::Index(InstrumentId::new(2)?),
            OptionTerms::new(
                OptionKind::Call,
                OptionPoints::new(100, 0)?,
                OptionPoints::new(5, 2)?,
                UsdPerPoint::new(100, 0)?,
                ExerciseStyle::European,
                SettlementStyle::Cash,
            )?,
            OptionSchedule::new(
                utc(0)?,
                utc(9)?,
                utc(10)?,
                utc(11)?,
                utc(12)?,
                OptionSchedule::timezone_rules().1,
            )?,
            EffectiveInterval::new(utc(0)?, utc(20)?)?,
        )?,
    ))
}
pub struct Fixture {
    tracker: ContinuityTracker,
    readiness: ReadinessPolicy,
    pub settlement: SettlementPolicy,
}
impl Fixture {
    pub fn new() -> Result<Self> {
        let contract = EvidenceContract::new(
            EvidenceKind::OptionSnapshot,
            MarketScope::Consolidated,
            rules()?,
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
        let instrument = InstrumentId::new(10)?;
        let observation = CanonicalObservation::new(
            ObservationContext::new(instrument, provider, transformation, session, &[])?,
            ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
            EvidenceTimes::new(at(2)?, None, None, at(2)?)?,
            None,
            Some(ObservationValue::OptionQuote(OptionQuoteSnapshot::new(
                &spec()?,
                QuoteSide::new(
                    OptionPoints::new(55, 2)?,
                    Some(DisplayedContracts::new(4)),
                    at(2)?,
                ),
                QuoteSide::new(
                    OptionPoints::new(60, 2)?,
                    Some(DisplayedContracts::new(4)),
                    at(2)?,
                ),
            )?)),
        )?;
        let mut tracker = ContinuityTracker::new(
            FeedScope::new(FeedId::new(1)?, provider, transformation, session),
            FeedEpoch::new(1)?,
            ContinuityMode::Snapshots,
            ContinuityPolicy::new(
                rules()?,
                8,
                2,
                Duration::from_secs(20),
                Duration::from_secs(20),
            )?,
            at(2)?,
        )?;
        tracker.ingest(FeedEpoch::new(1)?, at(2)?, observation)?;
        tracker.heartbeat(FeedEpoch::new(1)?, at(3)?, None)?;
        let readiness = ReadinessPolicy::new(
            rules()?,
            Duration::from_secs(20),
            Duration::ZERO,
            &[EvidenceRequirement::new(
                FeedId::new(1)?,
                instrument,
                contract,
                ContinuityRequirement::Snapshots,
            )],
        )?;
        let settlement = SettlementPolicy::new(
            spec()?,
            rules()?,
            SourceId::new(2)?,
            at(11)?,
            at(12)?,
            SettlementRounding::NearestEven,
            SettlementCharges::new(FeeBasis::EveryContract, 2),
        )?;
        Ok(Self {
            tracker,
            readiness,
            settlement,
        })
    }
    pub fn manifest(&self) -> Result<RunManifest> {
        Ok(RunManifest::new(
            at(0)?,
            digest(b"synthetic-code-v1"),
            digest(b"synthetic-population-calendar-strategy-v1"),
            [3; 32],
            &FrozenInput::from_ready(&self.ready()?, &[digest(b"synthetic-option-quote")])?,
        )?)
    }
    fn ready(&self) -> Result<ReadyEvidence<'_>> {
        Ok(self.readiness.check(
            at(3)?,
            &[EvidenceInput::new(
                &self.tracker,
                FeedEpoch::new(1)?,
                ObservationId::new(1)?,
                ObservationRevision::new(1)?,
            )],
        )?)
    }
    pub fn profiles(&self) -> Result<Vec<ExecutionProfile>> {
        [
            (10_000, 0, 1),
            (10_000, 1_000_000, 1),
            (10_000, 0, 4),
            (2_500, 0, 1),
        ]
        .into_iter()
        .map(|(bps, fee, seconds)| {
            Ok(ExecutionProfile::new(
                rules()?,
                bps,
                fee,
                0,
                Duration::from_secs(seconds),
                Duration::ZERO,
            )?)
        })
        .collect()
    }
    pub fn cases(&self) -> Result<Vec<EvaluationCase>> {
        let mut retained = self.settlement.canonical_bytes();
        retained.extend(self.final_fixing()?.canonical_bytes());
        let policy_evidence = digest(&retained);
        Ok(self
            .profiles()?
            .into_iter()
            .zip([
                SensitivityAxis::Baseline,
                SensitivityAxis::Cost,
                SensitivityAxis::Latency,
                SensitivityAxis::Liquidity,
            ])
            .enumerate()
            .map(|(i, (p, axis))| {
                EvaluationCase::new(
                    axis,
                    p,
                    digest(&[i as u8, 1]),
                    digest(&[i as u8, 2]),
                    Some((policy_evidence, digest(&[i as u8, 3]))),
                    2,
                )
            })
            .collect())
    }
    pub fn execute(
        &self,
        manifest: &RunManifest,
        index: usize,
        restart: bool,
    ) -> Result<OwnedTrace> {
        let profile = *self.profiles()?.get(index).ok_or("case")?;
        let mut run = Run::new(manifest);
        // Mirror account bootstrap's retained deterministic quote-ID allocation.
        let _bootstrap_id = run.next_id()?;
        let mut g = AccountAuditFrame::decode(include_bytes!("../fixtures/genesis-v1.json"))?
            .state()
            .clone();
        g.account = format!("{:032x}", 1);
        g.valuations[0].strategy = format!("{:032x}", 1);
        g.run = hex(manifest.digest());
        g.configuration = hex(digest(&[index as u8, 1]));
        g.execution_plan = Some(hex(digest(&[index as u8, 2])));
        g.settlement_plan = Some(hex(digest(&[index as u8, 3])));
        g.cash_cents = 100_000;
        g.positions.clear();
        g.valuations[0].valued_seconds = 0;
        let mut frames = vec![AccountAuditFrame::new(g.clone(), None)?];
        let mut checkpoints = vec![run.checkpoint()];
        let quantity = OrderQuantity::contracts(ContractQuantity::new(3)?);
        let price = OrderPrice::Premium(OptionPoints::new(60, 2)?);
        let definition = OrderDefinition::new(
            DryRunMode::parse(Some("dry_run"))?,
            OrderScope::new(
                manifest.digest(),
                RiskAccountId::new(1)?,
                RiskStrategyId::new(1)?,
                OrderId::new(run.next_id()?),
            ),
            OrderInstrument::index_option(spec()?)?,
            OrderSide::BuyToOpen,
            OrderKind::Limit(price),
            quantity,
            EvidenceInterval::new(at(0)?, at(9)?)?,
        )?;
        let mut order = Order::new(definition, 8)?;
        order.apply(OrderEvent::new(
            OrderEventId::new(run.next_id()?),
            order.definition().scope().identity().3,
            1,
            at(0)?,
            OrderEventKind::Accepted,
        )?)?;
        run.next_input();
        let ready = self.ready()?;
        let market = ExecutableLiquidity::from_ready(
            &run,
            order.definition(),
            profile,
            &ready,
            &[digest(b"synthetic-option-quote")],
        )?;
        let mut model = LiquidityExecution::new(market, 0, price, at(0)?)?;
        let attempt = OrderEventId::new(run.next_id()?);
        let fact = order.simulate(
            &run,
            &mut model,
            SimulationStep::new(
                attempt,
                2,
                &ready,
                profile.identity(),
                digest(b"synthetic-option-quote"),
                &[digest(b"synthetic-option-quote")],
            )?,
        )?;
        let count = match fact.effect().1 {
            OrderEventKind::Filled(fill) => fill.evidence().0.values().0.parts().1,
            _ => 0,
        };
        order.apply(fact)?;
        let (mut position, posting) = if count > 0 {
            FillAccounting::calculate(
                order.definition(),
                profile,
                price,
                count,
                (0, 0),
                10,
                PositionCost::new(0, 0)?,
            )?
            .effects()
        } else {
            (PositionCost::new(0, 0)?, [0; 4])
        };
        g.orders.push(AuditOrder {
            order: hex(ArtifactDigest::from_bytes(
                order.definition().scope().identity().3.bytes(),
            )),
            strategy: g.valuations[0].strategy.clone(),
            instrument: format!("{:032x}", 10),
            unit: "contracts".into(),
            side: "buy_to_open".into(),
            revision: 2,
            status: if count == 3 {
                "filled"
            } else if count > 0 {
                "partially_filled"
            } else {
                "working"
            }
            .into(),
            quantity: 3,
            filled: count,
            reserved_cash_cents: 0,
            reserved_units: 0,
        });
        if count > 0 {
            g.positions.push(AuditPosition {
                strategy: g.valuations[0].strategy.clone(),
                instrument: format!("{:032x}", 10),
                unit: "contracts".into(),
                quantity: count,
                cost_cents: Some(position.values().1),
            });
        }
        advance(&mut g, posting)?;
        frames.push(AccountAuditFrame::new(
            g.clone(),
            Some(fingerprint(frames.last().ok_or("genesis")?)?),
        )?);
        checkpoints.push(run.checkpoint());
        if restart {
            run = Run::restore(manifest, checkpoints.last().ok_or("checkpoint")?)?;
            let genesis = frames.first().ok_or("genesis")?;
            let mut view = AccountProjection::new(genesis.clone(), 3)?;
            view.apply(frames.last().ok_or("committed fill")?.clone())?;
            let resumed = AccountProjection::restore(genesis, &view.checkpoint()?)?;
            // Discard local financial caches: settle only restored audited state.
            g = resumed.state().clone();
            position = match g.positions.first() {
                Some(p) => PositionCost::new(p.quantity, p.cost_cents.ok_or("unknown cost")?)?,
                None => PositionCost::new(0, 0)?,
            };
        }
        run.advance_to(at(12)?)?;
        let fixing = self.final_fixing()?;
        let effect =
            match self
                .settlement
                .calculate_account(run.now(), position, g.cash_cents, &[fixing])?
            {
                SettlementDecision::Calculated(effect) => effect,
                _ => return Err("settlement deferred".into()),
            };
        for p in &mut g.positions {
            p.quantity = 0;
            p.cost_cents = Some(0);
        }
        for o in &mut g.orders {
            if o.status != "filled" {
                o.status = "expired".into();
                o.revision += 1;
            }
        }
        advance(&mut g, effect.effects().1)?;
        frames.push(AccountAuditFrame::new(
            g,
            Some(fingerprint(frames.last().ok_or("fill")?)?),
        )?);
        checkpoints.push(run.checkpoint());
        Ok(OwnedTrace {
            frames,
            checkpoints,
        })
    }
    fn final_fixing(&self) -> Result<SyntheticFinalFixing> {
        Ok(SyntheticFinalFixing::new(
            &self.settlement,
            ObservationId::new(2)?,
            at(12)?,
            IndexPoints::new(105, 0)?,
        )?)
    }
}
fn advance(g: &mut AccountAuditState, posting: [i128; 4]) -> Result {
    assert_eq!(posting.iter().sum::<i128>(), 0);
    g.cash_cents = u64::try_from(i128::from(g.cash_cents) + posting[0])?;
    for (total, delta) in g.postings.iter_mut().zip(posting) {
        *total = i64::try_from(i128::from(*total) + delta)?;
    }
    g.revision += 1;
    g.input_revision += 1;
    g.risk_revision += 1;
    Ok(())
}
pub struct OwnedTrace {
    pub frames: Vec<AccountAuditFrame>,
    pub checkpoints: Vec<Vec<u8>>,
}
impl OwnedTrace {
    pub fn samples(&self) -> Vec<EvaluationSample<'_>> {
        self.frames
            .iter()
            .zip(&self.checkpoints)
            .map(|(frame, checkpoint)| EvaluationSample { frame, checkpoint })
            .collect()
    }
}
