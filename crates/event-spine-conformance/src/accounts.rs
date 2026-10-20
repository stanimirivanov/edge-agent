//! Invented finite P08 account fixtures, never market/provider qualification.
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    simulation::{DEFINITIONS, SubmitDryRunOrder},
};
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{
    ArtifactDigest, FrozenInput, Run, RunManifest, admission::SyntheticReservationQuote, orders::*,
    risk::*,
};
use edgeagent_simulation_postgres::{
    AdmissionContext, AdmissionLimits, InitialInventory, InitialValuation, hex,
};
use std::time::Duration;
mod execution;

/// Qualification error; worker binaries never print private source chains.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
/// Fixed synthetic scenario. No hidden financial or operational defaults.
pub struct Scenario {
    manifest: RunManifest,
    policy: RiskPolicy,
    session: RiskSession,
    quotes: Vec<SyntheticReservationQuote>,
    case: String,
    tracker: ContinuityTracker,
    readiness: ReadinessPolicy,
}
/// Exact invented virtual time; no wall-clock reads.
pub fn at(second: i64) -> Result<PreciseInstant, Error> {
    Ok(PreciseInstant::from_unix_parts(second, 0)?)
}
fn rules() -> Result<SemanticReference, Error> {
    Ok(SemanticReference::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
    ))
}
impl Scenario {
    /// Build a named finite cash/inventory/options/risk/stale qualification fixture.
    pub fn new(case: &str) -> Result<Self, Error> {
        if !matches!(
            case,
            "cash"
                | "inventory"
                | "options"
                | "account-risk"
                | "strategy-risk"
                | "stale"
                | "valuation-stale"
                | "execution"
                | "execution-sell"
                | "execution-options"
        ) {
            return Err("unknown synthetic case".into());
        }
        let (manifest, tracker, readiness) = manifest(case)?;
        let session = RiskSession::new(
            SessionDate::new(1970, 1, 1)?,
            EvidenceInterval::new(at(0)?, at(100)?)?,
            rules()?,
        );
        let account_limits = RiskLimits::new(
            UsdCents::new(if case == "account-risk" {
                10_000
            } else {
                1_000_000
            }),
            UsdCents::new(100_000),
            10,
        );
        let strategy_limits = RiskLimits::new(UsdCents::new(1_000_000), UsdCents::new(100_000), 10);
        let first_limits = if case == "strategy-risk" {
            RiskLimits::new(UsdCents::new(5_000), UsdCents::new(100_000), 10)
        } else {
            strategy_limits
        };
        let policy = RiskPolicy::new(
            rules()?,
            Duration::from_secs(5),
            account_limits,
            &[
                (RiskStrategyId::new(1)?, first_limits),
                (RiskStrategyId::new(2)?, strategy_limits),
            ],
            &[RiskOperatorId::new(1)?],
        )?;
        let mut run = Run::new(&manifest);
        let mut quotes = Vec::new();
        for sequence in 1_u64..=2 {
            let strategy = if case == "inventory" || case == "execution-sell" {
                1
            } else {
                u128::from(sequence)
            };
            let side = if case == "inventory" || case == "execution-sell" {
                OrderSide::SellToClose
            } else {
                OrderSide::BuyToOpen
            };
            let (instrument, quantity, ceiling) =
                if case == "options" || case == "execution-options" {
                    (
                        OrderInstrument::index_option(option()?)?,
                        OrderQuantity::contracts(ContractQuantity::new(
                            if case.starts_with("execution") { 2 } else { 1 },
                        )?),
                        OrderPrice::Premium(OptionPoints::new(60, 2)?),
                    )
                } else {
                    (
                        OrderInstrument::equity(InstrumentId::new(1)?),
                        OrderQuantity::shares(if case == "inventory" {
                            4
                        } else if case.starts_with("execution") {
                            2
                        } else {
                            1
                        })?,
                        OrderPrice::PerShare(UsdPrice::new(60, 0)?),
                    )
                };
            let order = OrderDefinition::new(
                DryRunMode::parse(Some("dry_run"))?,
                OrderScope::new(
                    manifest.digest(),
                    RiskAccountId::new(1)?,
                    RiskStrategyId::new(strategy)?,
                    OrderId::new(run.next_id()?),
                ),
                instrument,
                side,
                OrderKind::Market,
                quantity,
                EvidenceInterval::new(at(1)?, at(100)?)?,
            )?;
            let end = if case == "stale" { 1 } else { 100 };
            let decision = if case == "valuation-stale" {
                10 + i64::try_from(sequence)?
            } else {
                1
            };
            quotes.push(SyntheticReservationQuote::new(
                order,
                ceiling,
                UsdCents::new(if case.starts_with("execution") { 2 } else { 0 }),
                EvidenceInterval::new(at(0)?, at(end)?)?,
                sequence,
                at(decision)?,
            )?);
        }
        Ok(Self {
            manifest,
            policy,
            session,
            quotes,
            case: case.into(),
            tracker,
            readiness,
        })
    }
    /// Bind a test principal outside the wire payload; only principal 1 is seeded.
    pub fn context(&self, principal: u128) -> Result<AdmissionContext<'_>, Error> {
        let slots = if self.case.starts_with("execution") {
            vec![
                (3, at(1)?),
                (4, at(2)?),
                (5, at(2)?),
                (6, at(2)?),
                (7, at(3)?),
                (8, at(4)?),
                (9, at(100)?),
            ]
        } else {
            vec![]
        };
        Ok(AdmissionContext::new_scheduled(
            RiskAccountId::new(1)?,
            principal,
            &self.manifest,
            self.policy.clone(),
            self.session,
            (&self.quotes, &slots),
            AdmissionLimits::new(Duration::from_secs(5), Duration::from_secs(15))?,
        )?)
    }
    /// Explicit invented cash, not a production account default.
    pub fn funding(&self) -> UsdCents {
        UsdCents::new(if self.case.starts_with("execution") {
            30_000
        } else if self.case == "account-risk" {
            100_000
        } else {
            10_000
        })
    }
    /// Complete synthetic valuation set, including inactive/zero allocations.
    pub fn valuations(&self) -> Result<Vec<InitialValuation>, Error> {
        (1..=2)
            .map(|id| {
                Ok(InitialValuation {
                    strategy: RiskStrategyId::new(id)?,
                    position_gross: UsdCents::new(
                        if (self.case == "inventory" || self.case == "execution-sell") && id == 1 {
                            30_000
                        } else {
                            0
                        },
                    ),
                    session_loss: UsdCents::new(0),
                    valued: at(0)?,
                    artifact: ArtifactDigest::of(b"invented-valuation-and-baseline-v1"),
                })
            })
            .collect()
    }
    /// Existing strategy-owned long shares for the inventory race only.
    pub fn inventory(&self) -> Result<Vec<InitialInventory>, Error> {
        Ok(
            if self.case == "inventory" || self.case == "execution-sell" {
                vec![InitialInventory {
                    strategy: RiskStrategyId::new(1)?,
                    instrument: InstrumentId::new(1)?,
                    unit: QuantityUnit::Shares,
                    quantity: 5,
                }]
            } else {
                vec![]
            },
        )
    }
    /// Stable request preserving its original optimistic preconditions.
    pub fn request(&self, index: usize) -> Result<SubmitDryRunOrder, Error> {
        let quote = self.quotes.get(index).ok_or("quote absent")?;
        Ok(SubmitDryRunOrder::new(
            "dry_run",
            &format!("{:032x}", 1),
            &hex(&quote.order().scope().identity().3.bytes()),
            &hex(&quote.fingerprint().bytes()),
            quote.position().0 - 1,
            quote.position().0,
        )?)
    }
    /// Gateway fixture envelope; changing message IDs preserves semantic identity.
    pub fn envelope(&self, index: usize, id: &str) -> Result<MessageEnvelope, Error> {
        Self::wrap(&self.request(index)?, id)
    }
    /// Wrap a validated command. This is not an authenticated public ingress.
    pub fn wrap(request: &SubmitDryRunOrder, id: &str) -> Result<MessageEnvelope, Error> {
        let account = request.identity().0;
        Ok(MessageEnvelope::from_payload(
            MessageMetadata {
                id: id.into(),
                source: Component::Gateway.source_uri().into(),
                message_type: DEFINITIONS[0].message_type().into(),
                subject: format!("account/{account}"),
                time: "1970-01-01T00:00:01Z".into(),
                data_schema: DEFINITIONS[0].data_schema().into(),
                correlation_id: "p08-synthetic".into(),
                causation_id: "p08-synthetic".into(),
                idempotency_key: request.identity().1.into(),
                partition_key: format!("account/{account}"),
                trace_parent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
                trace_state: None,
            },
            request,
        )?)
    }
}
fn manifest(case: &str) -> Result<(RunManifest, ContinuityTracker, ReadinessPolicy), Error> {
    let contract = EvidenceContract::new(
        if case == "execution-options" {
            EvidenceKind::OptionSnapshot
        } else {
            EvidenceKind::ShareTrade
        },
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
    let transform = TransformationRevision::new(1)?;
    let epoch = FeedEpoch::new(1)?;
    let observation = CanonicalObservation::new(
        ObservationContext::new(
            InstrumentId::new(if case == "execution-options" { 10 } else { 1 })?,
            provider,
            transform,
            session,
            &[],
        )?,
        ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
        EvidenceTimes::new(at(0)?, None, None, at(0)?)?,
        None,
        Some(if case == "execution-options" {
            ObservationValue::OptionQuote(OptionQuoteSnapshot::new(
                &option()?,
                QuoteSide::new(
                    OptionPoints::new(55, 2)?,
                    Some(DisplayedContracts::new(1)),
                    at(0)?,
                ),
                QuoteSide::new(
                    OptionPoints::new(60, 2)?,
                    Some(DisplayedContracts::new(1)),
                    at(0)?,
                ),
            )?)
        } else {
            ObservationValue::Trade(ShareTrade::new(
                UsdPrice::new(60, 0)?,
                ShareVolume::new(1, 0)?,
            )?)
        }),
    )?;
    let mut tracker = ContinuityTracker::new(
        FeedScope::new(FeedId::new(1)?, provider, transform, session),
        epoch,
        ContinuityMode::Snapshots,
        ContinuityPolicy::new(
            rules()?,
            4,
            1,
            Duration::from_secs(5),
            Duration::from_secs(5),
        )?,
        at(0)?,
    )?;
    tracker.ingest(epoch, at(0)?, observation)?;
    tracker.heartbeat(epoch, at(1)?, None)?;
    let policy = ReadinessPolicy::new(
        rules()?,
        Duration::from_secs(5),
        Duration::ZERO,
        &[EvidenceRequirement::new(
            FeedId::new(1)?,
            InstrumentId::new(if case == "execution-options" { 10 } else { 1 })?,
            contract,
            ContinuityRequirement::Snapshots,
        )],
    )?;
    let ready = policy.check(
        at(1)?,
        &[EvidenceInput::new(
            &tracker,
            epoch,
            ObservationId::new(1)?,
            ObservationRevision::new(1)?,
        )],
    )?;
    let manifest = RunManifest::new(
        at(0)?,
        ArtifactDigest::of(b"p08-synthetic-code-v1"),
        ArtifactDigest::of(case.as_bytes()),
        [0; 32],
        &FrozenInput::from_ready(&ready, &[ArtifactDigest::of(b"invented-share-trade-v1")])?,
    )?;
    Ok((manifest, tracker, policy))
}
fn option() -> Result<ReferenceVersion<OptionSpecification>, Error> {
    let utc = |second: i64| UtcInstant::from_unix_millis(second * 1000);
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
                utc(100)?,
                utc(120)?,
                utc(120)?,
                utc(121)?,
                OptionSchedule::timezone_rules().1,
            )?,
            EffectiveInterval::new(utc(0)?, utc(130)?)?,
        )?,
    ))
}
