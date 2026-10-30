//! Compiled, invented daily-v1 profile. No arbitrary observations or vendor factories.
use super::*;
use chrono::Datelike;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_replay::{OfflineMode, SyntheticDailySource, synthetic_daily_bytes};
use edgeagent_simulation_domain::ArtifactDigest;
use std::num::NonZeroU32;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Recipe {
    Single,
    History,
    PolicyHistory,
    Missing,
    ActionMismatch,
}
impl Recipe {
    pub fn name(self) -> &'static str {
        match self {
            Self::Single => "single-v1",
            Self::History => "history-v1",
            Self::PolicyHistory => "policy-history-v1",
            Self::Missing => "history-missing-v1",
            Self::ActionMismatch => "history-action-mismatch-v1",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        [
            Self::Single,
            Self::History,
            Self::PolicyHistory,
            Self::Missing,
            Self::ActionMismatch,
        ]
        .into_iter()
        .find(|r| r.name() == name)
        .ok_or_else(|| "unsupported invented recipe".into())
    }
    fn sessions(self) -> u8 {
        if self == Self::Single { 1 } else { 6 }
    }
}
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
    buckets: Vec<(SessionDate, EvidenceInterval)>,
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
    #[cfg(test)]
    pub fn new(id: u128, admitted_ms: i64) -> Result<Self> {
        Self::with_recipe(id, admitted_ms, Recipe::Single)
    }
    pub fn with_recipe(id: u128, admitted_ms: i64, recipe: Recipe) -> Result<Self> {
        let admitted = at(admitted_ms)?;
        let start_ms = admitted_ms
            .div_euclid(DAY)
            .checked_mul(DAY)
            // Daily quality requires twelve hours after bucket end. Two UTC dates
            // behind admission is eligible at every time of day, including midnight.
            .and_then(|v| v.checked_sub((i64::from(recipe.sessions()) + 1) * DAY))
            .ok_or("invalid synthetic date")?;
        let date =
            chrono::DateTime::from_timestamp_millis(start_ms).ok_or("invalid synthetic date")?;
        let end_date = date
            .date_naive()
            .checked_add_days(chrono::Days::new(u64::from(recipe.sessions())))
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
        let mut calendar_days = Vec::new();
        let mut buckets = Vec::new();
        for index in 0..recipe.sessions() {
            let start = start_ms + i64::from(index) * DAY;
            let date =
                chrono::DateTime::from_timestamp_millis(start).ok_or("invalid synthetic date")?;
            let session = SessionDate::new(date.year(), date.month(), date.day())?;
            calendar_days.push(ReferenceVersion::new(
                ReferenceId::new(u128::from(index) + 1)?,
                ReferenceRevision::new(1)?,
                SourceId::new(1)?,
                admitted,
                [7; 32],
                CalendarDay::new(
                    Venue::Xnys,
                    session,
                    SessionHours::Regular(SessionWindow::new(
                        at(start + 14 * 3_600_000)?,
                        at(start + 21 * 3_600_000)?,
                    )?),
                    "invented-session-v1",
                )?,
            ));
            buckets.push((
                session,
                EvidenceInterval::new(
                    PreciseInstant::from_millis(at(start)?),
                    PreciseInstant::from_millis(at(start + DAY)?),
                )?,
            ));
        }
        let calendar = ReferenceHistory::new(calendar_days)?;
        let actions = ReferenceHistory::new(if recipe == Recipe::ActionMismatch {
            vec![version(
                CorporateAction::new(scope.instrument, day, ActionKind::Unsupported),
                admitted,
            )?]
        } else {
            vec![]
        })?;
        let coverage = version(
            ActionCoverage::new(scope.instrument, range, at(admitted_ms + DAY)?),
            admitted,
        )?;
        let clock = PreciseInstant::from_millis(admitted);
        let feed = FeedId::new(1)?;
        let epoch = FeedEpoch::new(1)?;
        let mut trackers = Vec::new();
        for (index, (session, bucket)) in buckets.iter().enumerate() {
            if recipe == Recipe::Missing && index == 2 {
                continue;
            }
            let observation = CanonicalObservation::new(
                ObservationContext::new(
                    scope.instrument,
                    capability,
                    TransformationRevision::new(1)?,
                    *session,
                    &[],
                )?,
                ObservationVersion::new(
                    ObservationId::new(index as u128 + 1)?,
                    ObservationRevision::new(1)?,
                    None,
                )?,
                EvidenceTimes::new(
                    bucket.bounds().0,
                    None,
                    None,
                    PreciseInstant::from_millis(admitted),
                )?,
                None,
                Some(ObservationValue::Bar(RawBar::new(
                    *bucket,
                    BarCoverage::ProviderDay,
                    [UsdPrice::new(12345 + index as i128 * 100, 2)?; 4],
                    ShareVolume::new(0, 0)?,
                )?)),
            )?;
            let mut tracker = ContinuityTracker::new(
                FeedScope::new(
                    FeedId::new(index as u128 + 1)?,
                    capability,
                    TransformationRevision::new(1)?,
                    *session,
                ),
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
            trackers.push((index, tracker));
        }
        let requirements = trackers
            .iter()
            .map(|(index, _)| {
                Ok(EvidenceRequirement::new(
                    FeedId::new(*index as u128 + 1)?,
                    scope.instrument,
                    contract,
                    ContinuityRequirement::Snapshots,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let readiness = ReadinessPolicy::new(
            semantic,
            Duration::from_secs(if recipe == Recipe::Single {
                3 * 86_400
            } else {
                90 * 86_400
            }),
            Duration::from_secs(if recipe == Recipe::Single {
                86_400
            } else {
                6 * 86_400
            }),
            &requirements,
        )?;
        let inputs = trackers
            .iter()
            .map(|(index, tracker)| {
                Ok(EvidenceInput::new(
                    tracker,
                    epoch,
                    ObservationId::new(*index as u128 + 1)?,
                    ObservationRevision::new(1)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
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
        let retained = ready
            .observations()
            .iter()
            .map(|o| synthetic_daily_bytes(o).map_err(Into::into))
            .collect::<Result<Vec<_>>>()?;
        let implementation: &[u8] = if recipe == Recipe::Single {
            include_bytes!("../../fixtures/offline-single-v1/fixture.rs")
        } else if recipe == Recipe::PolicyHistory {
            include_bytes!("fixture.rs")
        } else {
            include_bytes!("../../fixtures/offline-history-v1/fixture.rs")
        };
        let mut config = selected.binding_bytes()?;
        if recipe == Recipe::PolicyHistory {
            config.extend_from_slice(include_bytes!(
                "../../fixtures/offline-policy-history-v1/events.json"
            ));
        }
        config.extend_from_slice(implementation);
        if recipe != Recipe::Single {
            config.extend_from_slice(recipe.name().as_bytes());
        }
        config.extend_from_slice(record.bytes());
        for bytes in &retained {
            config.extend_from_slice(bytes);
        }
        let source = SyntheticDailySource::new(
            OfflineMode,
            &selected,
            &ready,
            &retained,
            ArtifactDigest::of(implementation),
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
            buckets,
            quality,
        })
    }
    pub fn restore(record: &AdmissionRecord) -> Result<Self> {
        for recipe in [
            Recipe::Single,
            Recipe::History,
            Recipe::PolicyHistory,
            Recipe::Missing,
            Recipe::ActionMismatch,
        ] {
            let job = Self::with_recipe(
                record.request().id.get(),
                record.request().admitted_at.unix_millis(),
                recipe,
            )?;
            if job.record == *record {
                return Ok(job);
            }
        }
        Err("offline_configuration_mismatch".into())
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
        let series = DailySeriesSpec {
            instrument: self.record.request().scope.instrument,
            listing: ListingId::new(1)?,
            symbol: &self.symbol,
            venue: Venue::Xnys,
            range: self.record.request().range,
            cutoff: access.now(),
            provider: batch.provider,
            buckets: &self.buckets,
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
