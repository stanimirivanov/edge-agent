use crate::fixtures::*;
use edgeagent_market_data_domain::*;

pub struct Fixture {
    pub instrument: InstrumentId,
    pub listing: ListingId,
    pub symbol: Symbol,
    pub range: DateRange,
    pub cutoff: UtcInstant,
    pub known: UtcInstant,
    pub provider: ProviderCapability,
    pub policy: DailyQualityPolicy,
    pub buckets: Vec<(SessionDate, EvidenceInterval)>,
    pub mappings: ReferenceHistory<InstrumentMapping>,
    pub calendar: ReferenceHistory<CalendarDay>,
    pub actions: ReferenceHistory<CorporateAction>,
    pub coverage: ReferenceVersion<ActionCoverage>,
    pub bars: Vec<CanonicalObservation>,
}
pub fn provider(source: u128) -> TestResult<ProviderCapability> {
    Ok(ProviderCapability::new(
        SourceId::new(source)?,
        EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Venue(Venue::Xnys),
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::Milliseconds,
        )?,
        DeliveryCapability::Snapshots,
        KnowledgeCapability::CaptureOnly,
    ))
}
impl Fixture {
    pub fn new() -> TestResult<Self> {
        let known = utc(3, 1, 0, 0)?;
        let range = DateRange::new(date(3, 6)?, date(3, 10)?)?;
        let instrument = InstrumentId::new(1)?;
        let mut f = Self {
            instrument,
            listing: ListingId::new(1)?,
            symbol: Symbol::new("INVENTED")?,
            range,
            known,
            cutoff: utc(3, 10, 16, 0)?,
            provider: provider(1)?,
            policy: DailyQualityPolicy::new(
                TransformationRevision::new(1)?,
                31 * 86_400_000,
                1000,
                100,
            )?,
            buckets: vec![
                (
                    date(3, 6)?,
                    EvidenceInterval::new(
                        PreciseInstant::from_millis(utc(3, 6, 5, 0)?),
                        PreciseInstant::from_millis(utc(3, 7, 5, 0)?),
                    )?,
                ),
                (
                    date(3, 9)?,
                    EvidenceInterval::new(
                        PreciseInstant::from_millis(utc(3, 9, 4, 0)?),
                        PreciseInstant::from_millis(utc(3, 10, 4, 0)?),
                    )?,
                ),
            ],
            mappings: ReferenceHistory::new(vec![version(
                1,
                1,
                known,
                mapping(1, date(3, 1)?, date(4, 1)?)?,
            )?])?,
            calendar: ReferenceHistory::new(vec![
                version(
                    1,
                    1,
                    known,
                    day(3, 6, SessionHours::Regular(window(3, 6, 14, 21, 0)?))?,
                )?,
                version(2, 1, known, day(3, 7, SessionHours::Closed)?)?,
                version(3, 1, known, day(3, 8, SessionHours::Closed)?)?,
                version(
                    4,
                    1,
                    known,
                    day(3, 9, SessionHours::Regular(window(3, 9, 13, 20, 0)?))?,
                )?,
            ])?,
            actions: ReferenceHistory::new(vec![])?,
            coverage: version(
                1,
                1,
                known,
                ActionCoverage::new(instrument, range, utc(4, 1, 0, 0)?),
            )?,
            bars: vec![],
        };
        f.bars = vec![
            f.bar(1, 1, None, 0, 100, f.provider, None)?,
            f.bar(2, 1, None, 1, 100, f.provider, None)?,
        ];
        Ok(f)
    }
    pub fn spec(&self) -> DailySeriesSpec<'_> {
        DailySeriesSpec {
            instrument: self.instrument,
            listing: self.listing,
            symbol: &self.symbol,
            venue: Venue::Xnys,
            range: self.range,
            cutoff: self.cutoff,
            provider: self.provider,
            buckets: &self.buckets,
        }
    }
    pub fn references(&self) -> DailyReferences<'_> {
        DailyReferences {
            mappings: &self.mappings,
            calendar: &self.calendar,
            actions: &self.actions,
            action_coverage: &self.coverage,
        }
    }
    pub fn check<'a>(
        &'a self,
        bars: &'a [CanonicalObservation],
        other: Option<(ProviderCapability, &'a [CanonicalObservation])>,
    ) -> Result<CheckedDailySeries<'a>, DailyQualityError> {
        self.policy
            .check(&self.spec(), self.references(), bars, other)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bar(
        &self,
        id: u128,
        rev: u64,
        replaces: Option<u64>,
        index: usize,
        price: i128,
        provider: ProviderCapability,
        received: Option<UtcInstant>,
    ) -> TestResult<CanonicalObservation> {
        let (date, interval) = self.buckets[index];
        let receipt = received
            .map(PreciseInstant::from_millis)
            .unwrap_or(interval.bounds().1);
        Ok(CanonicalObservation::new(
            ObservationContext::new(
                self.instrument,
                provider,
                TransformationRevision::new(1)?,
                date,
                &[],
            )?,
            ObservationVersion::new(
                ObservationId::new(id)?,
                ObservationRevision::new(rev)?,
                replaces.map(ObservationRevision::new).transpose()?,
            )?,
            EvidenceTimes::new(interval.bounds().0, None, None, receipt)?,
            None,
            Some(ObservationValue::Bar(RawBar::new(
                interval,
                BarCoverage::ProviderDay,
                [UsdPrice::new(price, 0)?; 4],
                ShareVolume::new(0, 0)?,
            )?)),
        )?)
    }
}
pub fn replace_price(
    original: &CanonicalObservation,
    price: UsdPrice,
) -> TestResult<CanonicalObservation> {
    let Some(ObservationValue::Bar(bar)) = original.value() else {
        return Err("not a bar".into());
    };
    Ok(CanonicalObservation::new(
        original.context().clone(),
        original.version(),
        original.times(),
        None,
        Some(ObservationValue::Bar(RawBar::new(
            bar.bucket().0,
            bar.bucket().1,
            [price; 4],
            bar.values().1,
        )?)),
    )?)
}
