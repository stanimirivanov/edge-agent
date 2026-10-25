use super::*;
// An invented just-completed day exercises quality age without failing another
// admission/calendar invariant first. This is not an actual exchange schedule.
pub fn reject_recent_bucket() -> Result {
    let f = NativeFixture::new(1)?;
    let date = SessionDate::new(1970, 1, 2)?;
    let range = DateRange::new(date, SessionDate::new(1970, 1, 3)?)?;
    let calendar = version(
        70,
        1,
        at(0)?,
        CalendarDay::new(
            Venue::Xnys,
            date,
            SessionHours::Regular(SessionWindow::new(at(DAY + 3600000)?, at(DAY + 7200000)?)?),
            "synthetic-2026a",
        )?,
    )?;
    let interval = EvidenceInterval::new(
        PreciseInstant::from_millis(at(DAY)?),
        PreciseInstant::from_millis(now()?),
    )?;
    let rule = version(
        71,
        1,
        at(0)?,
        DateOnlyBucket::new(
            date,
            interval,
            SemanticReference::new(calendar.id(), calendar.revision()),
        )?,
    )?;
    let native = DateOnlyTime::new(
        date,
        rule,
        calendar.clone(),
        PreciseInstant::from_millis(now()?),
        now()?,
    )?;
    let observation = NativeDailyObservation::new(
        ObservationContext::new(
            f.f.request.instrument(),
            f.f.review.capability,
            TransformationRevision::new(8)?,
            date,
            &[],
        )?,
        f.versions[0],
        native,
        RawBar::new(
            interval,
            BarCoverage::ProviderDay,
            [UsdPrice::new(1, 0)?; 4],
            ShareVolume::new(0, 0)?,
        )?,
    )?;
    let symbol = Symbol::new("INVENTED")?;
    let mappings = ReferenceHistory::new(vec![version(
        4,
        1,
        at(0)?,
        InstrumentMapping::new(
            f.f.request.instrument(),
            ListingId::new(1)?,
            symbol.clone(),
            Venue::Xnys,
            range,
        ),
    )?])?;
    let calendars = ReferenceHistory::new(vec![calendar])?;
    let coverage = version(
        50,
        1,
        at(0)?,
        ActionCoverage::new(f.f.request.instrument(), range, at(NOW + DAY)?),
    )?;
    let mut spec = DailySeriesSpec {
        instrument: f.f.request.instrument(),
        listing: ListingId::new(1)?,
        symbol: &symbol,
        venue: Venue::Xnys,
        range,
        cutoff: now()?,
        provider: f.f.review.capability,
        buckets: &[(date, interval)],
    };
    let references = DailyReferences {
        mappings: &mappings,
        calendar: &calendars,
        actions: &f.actions,
        action_coverage: &coverage,
    };
    let observations = [observation];
    assert!(matches!(
        f.policy.check_native(&spec, references, &observations),
        Err(DailyQualityError::Bucket)
    ));
    spec.cutoff = at(NOW + 43_200_000 - 1)?;
    assert!(matches!(
        f.policy.check_native(&spec, references, &observations),
        Err(DailyQualityError::Bucket)
    ));
    spec.cutoff = at(NOW + 43_200_000)?;
    assert!(
        f.policy
            .check_native(&spec, references, &observations)
            .is_ok(),
        "the twelve-hour boundary is inclusive"
    );
    Ok(())
}
fn version<T>(id: u128, rev: u64, known: UtcInstant, value: T) -> Result<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(id)?,
        ReferenceRevision::new(rev)?,
        SourceId::new(1)?,
        known,
        [7; 32],
        value,
    ))
}
pub struct NativeFixture {
    pub f: Fixture,
    pub schedule: DateOnlySchedule,
    pub record: AdmissionRecord,
    pub capture: DateOnlyDailyArtifact,
    pub mappings: ReferenceHistory<InstrumentMapping>,
    pub calendar: ReferenceHistory<CalendarDay>,
    pub actions: ReferenceHistory<CorporateAction>,
    pub coverage: ReferenceVersion<ActionCoverage>,
    pub versions: Vec<ObservationVersion>,
    pub policy: DailyQualityPolicy,
    transformation: TransformationRevision,
}
impl NativeFixture {
    pub fn new(request: u128) -> Result<Self> {
        Self::at(request, NOW)
    }
    pub fn at(request: u128, ms: i64) -> Result<Self> {
        Self::with_start(request, ms, 30)
    }
    pub fn with_start(request: u128, ms: i64, first_day: u32) -> Result<Self> {
        let now = || at(ms);
        let known = at(ms - NOW)?;
        let mut f = Fixture::new()?;
        f.request.id = AcquisitionId::new(request)?;
        f.request.admitted_at = now()?;
        f.request.deadline = at(ms + 300_000)?;
        f.right.effective = EffectiveInterval::new(known, at(ms + 2 * DAY)?)?;
        f.right.delete_by = at(ms + 3 * DAY)?;
        f.review.effective = f.right.effective;
        f.request.contract = EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Consolidated,
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::DateOnly,
        )?;
        f.request.range = DateRange::new(
            SessionDate::new(1969, 12, first_day)?,
            SessionDate::new(1970, 1, 2)?,
        )?;
        f.right.contract = f.request.contract;
        f.review.capability = ProviderCapability::new(
            f.request.scope.source,
            f.request.contract,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        );
        f.rights = ReferenceHistory::new(vec![version(1, 1, known, f.right)?])?;
        f.providers = ReferenceHistory::new(vec![version(1, 1, known, f.review)?])?;
        let symbol = Symbol::new("INVENTED")?;
        let mapping = version(
            4,
            1,
            known,
            InstrumentMapping::new(
                f.request.instrument(),
                ListingId::new(1)?,
                symbol.clone(),
                Venue::Xnys,
                f.request.range,
            ),
        )?;
        let scope = RoutingScope {
            source: f.request.scope.source,
            feed: FeedId::new(1)?,
            epoch: FeedEpoch::new(1)?,
        };
        let effective = EffectiveInterval::new(known, at(ms + DAY)?)?;
        let directories = ReferenceHistory::new(vec![version(
            5,
            1,
            known,
            RoutingDirectory::new(
                scope,
                effective,
                vec![RouteBinding::new(
                    RoutingKey::symbol(&symbol),
                    mapping.clone(),
                )?],
            )?,
        )?])?;
        let selections = ReferenceHistory::new(vec![version(
            6,
            1,
            known,
            InstrumentSelection::new(effective, &[f.request.instrument()])?,
        )?])?;
        let plan = SelectionPlan::resolve(
            SelectionContext {
                scope,
                effective_at: now()?,
                cutoff: now()?,
                session: f.request.range.bounds().0,
            },
            (ReferenceId::new(6)?, ReferenceId::new(5)?),
            &selections,
            &directories,
        )?;
        let selected = plan.prepare(&f.request)?;
        let mut calendars = Vec::new();
        let mut buckets = Vec::new();
        let mut rows = Vec::new();
        let mut versions = Vec::new();
        for (offset, day) in [(0, 30), (1, 31)]
            .into_iter()
            .filter(|(_, day)| *day >= first_day)
        {
            let date = SessionDate::new(1969, 12, day)?;
            let start = -2 * DAY + offset * DAY;
            let calendar = version(
                u128::from(day),
                1,
                known,
                CalendarDay::new(
                    Venue::Xnys,
                    date,
                    SessionHours::Regular(SessionWindow::new(
                        at(start + 3600000)?,
                        at(start + 7200000)?,
                    )?),
                    "synthetic-2026a",
                )?,
            )?;
            buckets.push(version(
                u128::from(day) + 100,
                1,
                known,
                DateOnlyBucket::new(
                    date,
                    EvidenceInterval::new(
                        PreciseInstant::from_millis(at(start)?),
                        PreciseInstant::from_millis(at(start + DAY)?),
                    )?,
                    SemanticReference::new(calendar.id(), calendar.revision()),
                )?,
            )?);
            calendars.push(calendar);
            rows.push((
                date,
                [UsdPrice::new(100 + i128::from(offset), 2)?; 4],
                ShareVolume::new(0, 0)?,
            ));
            versions.push(ObservationVersion::new(
                ObservationId::new(u128::from(day))?,
                ObservationRevision::new(1)?,
                None,
            )?);
        }
        calendars.push(version(
            40,
            1,
            known,
            CalendarDay::new(
                Venue::Xnys,
                SessionDate::new(1970, 1, 1)?,
                SessionHours::Closed,
                "synthetic-2026a",
            )?,
        )?);
        let schedule = DateOnlySchedule::new(&selected, &calendars, &buckets)?;
        let policy =
            DailyQualityPolicy::new(TransformationRevision::new(7)?, (3 * DAY) as u64, 1000, 100)?;
        let admitted = f
            .catalogs()
            .admit(&f.request, now()?, &Cancellation::default())?;
        let record = AdmissionRecord::new(
            &admitted,
            AcquisitionLimits {
                attempts: 3,
                spacing_ms: 15000,
                starts_per_minute: 4,
            },
            ListingId::new(1)?,
            policy,
        )?
        .with_configuration(schedule.configuration_digest())?;
        let capture = schedule.capture(
            &record,
            &rows,
            DailyEvidenceArtifact::hash(b"invented-body"),
            now()?,
            now()?,
        )?;
        let coverage = version(
            50,
            1,
            known,
            ActionCoverage::new(f.request.instrument(), f.request.range, at(ms + DAY)?),
        )?;
        Ok(Self {
            f,
            schedule,
            record,
            capture,
            mappings: ReferenceHistory::new(vec![mapping])?,
            calendar: ReferenceHistory::new(calendars)?,
            actions: ReferenceHistory::new(vec![])?,
            coverage,
            versions,
            policy,
            transformation: TransformationRevision::new(8)?,
        })
    }
    pub fn input(&self) -> NativeEvidenceFreeze<'_> {
        NativeEvidenceFreeze {
            schedule: &self.schedule,
            capture: &self.capture,
            versions: &self.versions,
            transformation: self.transformation,
            references: DailyReferences {
                mappings: &self.mappings,
                calendar: &self.calendar,
                actions: &self.actions,
                action_coverage: &self.coverage,
            },
            policy: self.policy,
            previous: None,
        }
    }
    pub fn freeze(
        &self,
        input: NativeEvidenceFreeze<'_>,
    ) -> std::result::Result<NativeDailyArtifact, EvidenceFreezeError> {
        let admitted = self
            .f
            .catalogs()
            .admit(
                &self.f.request,
                now().map_err(|_| EvidenceFreezeError::Contract)?,
                &Cancellation::default(),
            )
            .map_err(EvidenceFreezeError::Acquisition)?;
        freeze_native_evidence(
            &self.record,
            &admitted,
            self.f.catalogs(),
            now().map_err(|_| EvidenceFreezeError::Contract)?,
            &Cancellation::default(),
            input,
        )
    }
}
