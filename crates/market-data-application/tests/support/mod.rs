use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::num::NonZeroU32;
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const DAY: i64 = 86_400_000;
pub const NOW: i64 = 2 * DAY;
pub fn at(ms: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
pub fn now() -> Result<UtcInstant> {
    at(NOW)
}
pub fn nz(value: u32) -> Result<NonZeroU32> {
    NonZeroU32::new(value).ok_or("zero fixture".into())
}
pub fn version<T>(id: u128, rev: u64, known: UtcInstant, value: T) -> Result<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(id)?,
        ReferenceRevision::new(rev)?,
        SourceId::new(9)?,
        known,
        [7; 32],
        value,
    ))
}
pub struct Fixture {
    pub request: AcquisitionRequest,
    pub right: RightsRecord,
    pub review: ProviderReview,
    pub rights: ReferenceHistory<RightsRecord>,
    pub providers: ReferenceHistory<ProviderReview>,
}
impl Fixture {
    pub fn new() -> Result<Self> {
        let scope = AccessScope {
            principal: PrincipalId::new(1)?,
            account: ProviderAccountId::new(1)?,
            source: SourceId::new(1)?,
            instrument: InstrumentId::new(1)?,
            endpoint: ReferenceId::new(1)?,
            audience: AudienceId::new(1)?,
            storage: StorageScopeId::new(1)?,
        };
        let contract = EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Venue(Venue::Xnys),
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::Milliseconds,
        )?;
        let effective = EffectiveInterval::new(at(0)?, at(NOW + 2 * DAY)?)?;
        let right = RightsRecord {
            scope,
            contract,
            agreement: ReferenceId::new(3)?,
            effective,
            permissions: UsePermissions {
                automated: Permission::Allowed,
                derived: Permission::Allowed,
                canonical_storage: Permission::Allowed,
                audience: Permission::Allowed,
            },
            raw_storage: Permission::Unknown,
            canonical_retention_ms: nz(u32::MAX)?,
            raw_retention_ms: None,
            delete_by: at(NOW + 3 * DAY)?,
            embargo_ms: 0,
            status: ReviewStatus::Active,
        };
        let review = ProviderReview {
            scope,
            capability: ProviderCapability::new(
                scope.source,
                contract,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly,
            ),
            effective,
            status: ReviewStatus::Active,
        };
        let request = AcquisitionRequest {
            id: AcquisitionId::new(1)?,
            scope,
            contract,
            range: DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 2)?)?,
            knowledge: KnowledgeMode::Capture,
            admitted_at: now()?,
            deadline: at(NOW + 300_000)?,
            raw: RawRetention::Discard,
            budget: RequestBudget {
                connect_ms: nz(5000)?,
                request_ms: nz(15000)?,
                calls: nz(12)?,
                pages: nz(4)?,
                response_bytes: nz(1_048_576)?,
                command_bytes: nz(8_388_608)?,
            },
        };
        Ok(Self {
            request,
            right,
            review,
            rights: ReferenceHistory::new(vec![version(1, 1, at(0)?, right)?])?,
            providers: ReferenceHistory::new(vec![version(1, 1, at(0)?, review)?])?,
        })
    }
    pub fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.providers,
        }
    }
    pub fn batch(&self, receipt: UtcInstant) -> Result<ProviderBatch> {
        let start = PreciseInstant::from_millis(at(0)?);
        let end = PreciseInstant::from_millis(at(DAY)?);
        let observation = CanonicalObservation::new(
            ObservationContext::new(
                self.request.scope.instrument,
                self.review.capability,
                TransformationRevision::new(1)?,
                SessionDate::new(1970, 1, 1)?,
                &[],
            )?,
            ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
            EvidenceTimes::new(start, None, None, PreciseInstant::from_millis(receipt))?,
            None,
            Some(ObservationValue::Bar(RawBar::new(
                EvidenceInterval::new(start, end)?,
                BarCoverage::ProviderDay,
                [UsdPrice::new(123456789, 4)?; 4],
                ShareVolume::new(0, 0)?,
            )?)),
        )?;
        Ok(ProviderBatch {
            request: self.request.id,
            provider: self.review.capability,
            observations: vec![observation],
        })
    }
}
pub struct Fake {
    pub batch: ProviderBatch,
    pub calls: std::sync::atomic::AtomicUsize,
}
impl DailyProvider for Fake {
    async fn fetch(
        &self,
        _: &AuthorizedAcquisition<'_>,
        cancel: &Cancellation,
    ) -> std::result::Result<ProviderBatch, ProviderFailure> {
        if cancel.is_cancelled() {
            return Err(ProviderFailure::Cancelled);
        }
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(ProviderBatch {
            request: self.batch.request,
            provider: self.batch.provider,
            observations: self.batch.observations.clone(),
        })
    }
}
pub fn ready<T>(future: impl std::future::Future<Output = T>) -> Result<T> {
    let mut pinned = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => Ok(result),
        std::task::Poll::Pending => Err("synthetic future unexpectedly pending".into()),
    }
}
