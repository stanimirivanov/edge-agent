//! Compiled, invented daily-v1 profile. No arbitrary observations or vendor factories.
use super::*;
use chrono::Datelike;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_replay::{OfflineMode, SyntheticDailySource, synthetic_daily_bytes};
use edgeagent_simulation_domain::ArtifactDigest;
use std::num::NonZeroU32;

const DAY: i64 = 86_400_000;
pub(super) struct Job {
    pub record: AdmissionRecord,
    pub source: SyntheticDailySource,
    pub rights: ReferenceHistory<RightsRecord>,
    pub providers: ReferenceHistory<ProviderReview>,
    mappings: ReferenceHistory<InstrumentMapping>,
    calendar: ReferenceHistory<CalendarDay>,
    actions: ReferenceHistory<CorporateAction>,
    coverage: ReferenceVersion<ActionCoverage>,
    symbol: Symbol,
    bucket: EvidenceInterval,
    day: SessionDate,
    quality: DailyQualityPolicy,
}

fn at(ms: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
fn version<T>(value: T, known: UtcInstant) -> Result<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        known,
        [7; 32],
        value,
    ))
}
impl Job {
    pub fn new(id: u128, admitted_ms: i64) -> Result<Self> {
        let admitted = at(admitted_ms)?;
        let start_ms = admitted_ms
            .div_euclid(DAY)
            .checked_mul(DAY)
            // Daily quality requires twelve hours after bucket end. Two UTC dates
            // behind admission is eligible at every time of day, including midnight.
            .and_then(|v| v.checked_sub(2 * DAY))
            .ok_or("invalid synthetic date")?;
        let date =
            chrono::DateTime::from_timestamp_millis(start_ms).ok_or("invalid synthetic date")?;
        let end_date = date
            .date_naive()
            .succ_opt()
            .ok_or("invalid synthetic date")?;
        let day = SessionDate::new(date.year(), date.month(), date.day())?;
        let range = DateRange::new(
            day,
            SessionDate::new(end_date.year(), end_date.month(), end_date.day())?,
        )?;
        let scope = AccessScope {
            principal: PrincipalId::new(1)?,
            account: ProviderAccountId::new(1)?,
            source: SourceId::new(1)?,
            instrument: InstrumentId::new(1)?,
            endpoint: ReferenceId::new(1)?,
            audience: AudienceId::new(1)?,
            storage: StorageScopeId::new(id)?,
        };
        let semantic = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
        let contract = EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Venue(Venue::Xnys),
            semantic,
            TimestampPrecision::Milliseconds,
        )?;
        let capability = ProviderCapability::new(
            scope.source,
            contract,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        );
        let effective = EffectiveInterval::new(
            at(start_ms)?,
            at(admitted_ms
                .checked_add(DAY)
                .ok_or("invalid synthetic date")?)?,
        )?;
        let nz = |v| NonZeroU32::new(v).ok_or("invalid synthetic bound");
        let rights = ReferenceHistory::new(vec![version(
            RightsRecord {
                scope,
                contract,
                agreement: ReferenceId::new(1)?,
                effective,
                permissions: UsePermissions {
                    automated: Permission::Allowed,
                    derived: Permission::Allowed,
                    canonical_storage: Permission::Allowed,
                    audience: Permission::Allowed,
                },
                raw_storage: Permission::Denied,
                canonical_retention_ms: nz(600_000)?,
                raw_retention_ms: None,
                delete_by: at(admitted_ms
                    .checked_add(600_000)
                    .ok_or("invalid synthetic date")?)?,
                embargo_ms: 0,
                status: ReviewStatus::Active,
            },
            admitted,
        )?])?;
        let providers = ReferenceHistory::new(vec![version(
            ProviderReview {
                scope,
                capability,
                effective,
                status: ReviewStatus::Active,
            },
            admitted,
        )?])?;
        let request = AcquisitionRequest {
            id: AcquisitionId::new(id)?,
            scope,
            contract,
            range,
            knowledge: KnowledgeMode::Capture,
            admitted_at: admitted,
            deadline: at(admitted_ms
                .checked_add(300_000)
                .ok_or("invalid synthetic date")?)?,
            raw: RawRetention::Discard,
            budget: RequestBudget {
                connect_ms: nz(5_000)?,
                request_ms: nz(15_000)?,
                calls: nz(12)?,
                pages: nz(4)?,
                response_bytes: nz(1_048_576)?,
                command_bytes: nz(8_388_608)?,
            },
        };
        let symbol = Symbol::new("INVENTED")?;
        let mapping = version(
            InstrumentMapping::new(
                scope.instrument,
                ListingId::new(1)?,
                symbol.clone(),
                Venue::Xnys,
                range,
            ),
            admitted,
        )?;
        let mappings = ReferenceHistory::new(vec![mapping.clone()])?;
        let calendar = ReferenceHistory::new(vec![version(
            CalendarDay::new(
                Venue::Xnys,
                day,
                SessionHours::Regular(SessionWindow::new(
                    at(start_ms + 14 * 3_600_000)?,
                    at(start_ms + 21 * 3_600_000)?,
                )?),
                "invented-session-v1",
            )?,
            admitted,
        )?])?;
        let actions = ReferenceHistory::new(vec![])?;
        let coverage = version(
            ActionCoverage::new(scope.instrument, range, at(admitted_ms + DAY)?),
            admitted,
        )?;
        let bucket = EvidenceInterval::new(
            PreciseInstant::from_millis(at(start_ms)?),
            PreciseInstant::from_millis(at(start_ms + DAY)?),
        )?;
        let observation = CanonicalObservation::new(
            ObservationContext::new(
                scope.instrument,
                capability,
                TransformationRevision::new(1)?,
                day,
                &[],
            )?,
            ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
            EvidenceTimes::new(
                bucket.bounds().0,
                None,
                None,
                PreciseInstant::from_millis(admitted),
            )?,
            None,
            Some(ObservationValue::Bar(RawBar::new(
                bucket,
                BarCoverage::ProviderDay,
                [UsdPrice::new(12345, 2)?; 4],
                ShareVolume::new(0, 0)?,
            )?)),
        )?;
        let clock = PreciseInstant::from_millis(admitted);
        let feed = FeedId::new(1)?;
        let epoch = FeedEpoch::new(1)?;
        let mut tracker = ContinuityTracker::new(
            FeedScope::new(feed, capability, TransformationRevision::new(1)?, day),
            epoch,
            ContinuityMode::Snapshots,
            ContinuityPolicy::new(
                semantic,
                8,
                2,
                Duration::from_secs(10),
                Duration::from_secs(10),
            )?,
            clock,
        )?;
        tracker.ingest(epoch, clock, observation)?;
        tracker.heartbeat(epoch, clock, None)?;
        let readiness = ReadinessPolicy::new(
            semantic,
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
            &[EvidenceRequirement::new(
                feed,
                scope.instrument,
                contract,
                ContinuityRequirement::Snapshots,
            )],
        )?;
        let inputs = [EvidenceInput::new(
            &tracker,
            epoch,
            ObservationId::new(1)?,
            ObservationRevision::new(1)?,
        )];
        let ready = readiness.check(clock, &inputs)?;
        let routing = RoutingScope {
            source: scope.source,
            feed,
            epoch,
        };
        let selection = ReferenceHistory::new(vec![version(
            InstrumentSelection::new(effective, &[scope.instrument])?,
            admitted,
        )?])?;
        let directory = ReferenceHistory::new(vec![version(
            RoutingDirectory::new(
                routing,
                effective,
                vec![RouteBinding::new(RoutingKey::symbol(&symbol), mapping)?],
            )?,
            admitted,
        )?])?;
        let plan = SelectionPlan::resolve(
            SelectionContext {
                scope: routing,
                effective_at: admitted,
                cutoff: admitted,
                session: day,
            },
            (ReferenceId::new(1)?, ReferenceId::new(1)?),
            &selection,
            &directory,
        )?;
        let selected = plan.prepare(&request)?;
        let quality = DailyQualityPolicy::new(TransformationRevision::new(1)?, 300_000, 1000, 100)?;
        let catalogs = AuthorityCatalogs {
            rights: &rights,
            providers: &providers,
        };
        let authority = catalogs.admit(&request, admitted, &Cancellation::default())?;
        let record = AdmissionRecord::new(
            &authority,
            AcquisitionLimits {
                attempts: 3,
                spacing_ms: 15_000,
                starts_per_minute: 4,
            },
            ListingId::new(1)?,
            quality,
        )?;
        let retained = vec![synthetic_daily_bytes(
            ready
                .observations()
                .first()
                .ok_or("missing invented input")?,
        )?];
        let mut config = selected.binding_bytes()?;
        config.extend_from_slice(include_bytes!("fixture.rs"));
        config.extend_from_slice(record.bytes());
        for bytes in &retained {
            config.extend_from_slice(bytes);
        }
        let source = SyntheticDailySource::new(
            OfflineMode,
            &selected,
            &ready,
            &retained,
            ArtifactDigest::of(include_bytes!("fixture.rs")),
            ArtifactDigest::of(&config),
            [0; 32],
        )?;
        let record = record.with_configuration(&DailyEvidenceArtifact::hash(&config))?;
        Ok(Self {
            record,
            source,
            rights,
            providers,
            mappings,
            calendar,
            actions,
            coverage,
            symbol,
            bucket,
            day,
            quality,
        })
    }
    pub fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.providers,
        }
    }
    pub async fn prepare(
        &self,
        storage: &impl EvidenceStorage,
        access: &impl PublicationAuthority,
        cancel: &Cancellation,
    ) -> Result<TerminalProposal> {
        let authority = self
            .catalogs()
            .admit(self.record.request(), access.now(), cancel)?;
        let batch = self.source.fetch(&authority, cancel).await?;
        let buckets = [(self.day, self.bucket)];
        let series = DailySeriesSpec {
            instrument: self.record.request().scope.instrument,
            listing: ListingId::new(1)?,
            symbol: &self.symbol,
            venue: Venue::Xnys,
            range: self.record.request().range,
            cutoff: access.now(),
            provider: batch.provider,
            buckets: &buckets,
        };
        Ok(TerminalProposal::ready(
            &self.record,
            &authority,
            self.catalogs(),
            cancel,
            EvidenceFreeze {
                command: self.record.command(),
                batch: &batch,
                series: &series,
                references: DailyReferences {
                    mappings: &self.mappings,
                    calendar: &self.calendar,
                    actions: &self.actions,
                    action_coverage: &self.coverage,
                },
                policy: self.quality,
                previous: None,
            },
            storage,
            access,
        )
        .await?)
    }
}
