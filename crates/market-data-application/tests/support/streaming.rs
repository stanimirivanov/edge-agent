//! Invented account, rights and selection only; no provider permission or transport.
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn at(ms: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
pub fn nz(n: u32) -> Result<NonZeroU32> {
    NonZeroU32::new(n).ok_or("zero fixture".into())
}
fn version<T>(id: u128, rev: u64, ms: i64, value: T) -> Result<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(id)?,
        ReferenceRevision::new(rev)?,
        SourceId::new(1)?,
        at(ms)?,
        [7; 32],
        value,
    ))
}
pub struct Fixture {
    pub ms: i64,
    pub scopes: Vec<AccessScope>,
    pub contract: EvidenceContract,
    pub rights: ReferenceHistory<RightsRecord>,
    pub providers: ReferenceHistory<ProviderReview>,
    selections: ReferenceHistory<InstrumentSelection>,
    directories: ReferenceHistory<RoutingDirectory>,
    context: SelectionContext,
}
impl Fixture {
    pub fn new(ms: i64, members: u32) -> Result<Self> {
        let contract = EvidenceContract::new(
            EvidenceKind::ShareTrade,
            MarketScope::Consolidated,
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::Milliseconds,
        )?;
        Self::profile(ms, members, contract, DeliveryCapability::Sequenced)
    }
    pub fn profile(
        ms: i64,
        members: u32,
        contract: EvidenceContract,
        delivery: DeliveryCapability,
    ) -> Result<Self> {
        let interval = EffectiveInterval::new(at(ms - 1000)?, at(ms + 86_400_000)?)?;
        let routing = RoutingScope {
            source: SourceId::new(1)?,
            feed: FeedId::new(1)?,
            epoch: FeedEpoch::new(1)?,
        };
        let mut scopes = Vec::new();
        let mut rights = Vec::new();
        let mut reviews = Vec::new();
        let mut routes = Vec::new();
        for n in 1..=members {
            let scope = AccessScope {
                principal: PrincipalId::new(1)?,
                account: ProviderAccountId::new(1)?,
                source: routing.source,
                instrument: InstrumentId::new(u128::from(n))?,
                endpoint: ReferenceId::new(1)?,
                audience: AudienceId::new(1)?,
                storage: StorageScopeId::new(1)?,
            };
            rights.push(version(
                u128::from(n) + 10,
                1,
                ms - 1,
                RightsRecord {
                    scope,
                    contract,
                    agreement: ReferenceId::new(1)?,
                    effective: interval,
                    permissions: UsePermissions {
                        automated: Permission::Allowed,
                        derived: Permission::Allowed,
                        canonical_storage: Permission::Allowed,
                        audience: Permission::Allowed,
                    },
                    raw_storage: Permission::Denied,
                    canonical_retention_ms: nz(86_400_000)?,
                    raw_retention_ms: None,
                    delete_by: at(ms + 86_400_000)?,
                    embargo_ms: 0,
                    status: ReviewStatus::Active,
                },
            )?);
            reviews.push(version(
                u128::from(n) + 100,
                1,
                ms - 1,
                ProviderReview {
                    scope,
                    capability: ProviderCapability::new(
                        scope.source,
                        contract,
                        delivery,
                        KnowledgeCapability::CaptureOnly,
                    ),
                    effective: interval,
                    status: ReviewStatus::Active,
                },
            )?);
            let symbol = Symbol::new(&format!("SYNTH{n}"))?;
            let mapping = InstrumentMapping::new(
                scope.instrument,
                ListingId::new(u128::from(n))?,
                symbol.clone(),
                Venue::Xnas,
                DateRange::new(
                    SessionDate::new(1969, 12, 31)?,
                    SessionDate::new(1970, 1, 2)?,
                )?,
            );
            routes.push(RouteBinding::new(
                RoutingKey::symbol(&symbol),
                version(u128::from(n) + 200, 1, ms - 1, mapping)?,
            )?);
            scopes.push(scope);
        }
        let instruments: Vec<_> = scopes.iter().map(|s| s.instrument).collect();
        let selection = InstrumentSelection::new(interval, &instruments)?;
        let directory = RoutingDirectory::new(routing, interval, routes)?;
        Ok(Self {
            ms,
            scopes,
            contract,
            rights: ReferenceHistory::new(rights)?,
            providers: ReferenceHistory::new(reviews)?,
            selections: ReferenceHistory::new(vec![version(500, 1, ms - 1, selection)?])?,
            directories: ReferenceHistory::new(vec![version(501, 1, ms - 1, directory)?])?,
            context: SelectionContext {
                scope: routing,
                effective_at: at(ms)?,
                cutoff: at(ms)?,
                session: SessionDate::new(1970, 1, 1)?,
            },
        })
    }
    pub fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.providers,
        }
    }
    pub fn account(&self) -> Result<ProviderAccountId> {
        Ok(ProviderAccountId::new(1)?)
    }
    pub fn policy(&self, connections: u16, starts: u32) -> Result<StreamingAccountPolicy> {
        Ok(StreamingAccountPolicy {
            revision: nz(1)?,
            connections: NonZeroU16::new(connections).ok_or("zero fixture")?,
            starts: nz(starts)?,
            session_ms: nz(3_600_000)?,
            spacing_ms: nz(1)?,
            expires_at: at(self.ms + 7_200_000)?,
        })
    }
    pub fn data(&self, id: u128) -> Result<StreamingSessionData> {
        Ok(StreamingSessionData {
            id: EvidenceId::new(&format!("{id:032x}"))?,
            scopes: self.scopes.clone(),
            contract: self.contract,
            delivery: DeliveryCapability::Sequenced,
            window: EffectiveInterval::new(at(self.ms)?, at(self.ms + 3_600_000)?)?,
            configuration: DailyEvidenceArtifact::hash(b"invented complete configuration"),
            limits: StreamingLimits {
                starts: nz(2)?,
                frames: nz(3)?,
                frame_bytes: nz(10)?,
                bytes: NonZeroU64::new(20).ok_or("zero fixture")?,
                subscriptions: NonZeroU16::new(32).ok_or("zero fixture")?,
                connect_ms: nz(100)?,
                idle_ms: nz(500)?,
            },
        })
    }
    pub fn seal(&self, data: StreamingSessionData) -> Result<StreamingSession> {
        let plan = self.plan()?;
        Ok(StreamingSession::new(
            data,
            &plan,
            self.catalogs(),
            at(self.ms)?,
        )?)
    }
    pub fn plan(&self) -> Result<SelectionPlan<'_>> {
        Ok(SelectionPlan::resolve(
            self.context,
            (ReferenceId::new(500)?, ReferenceId::new(501)?),
            &self.selections,
            &self.directories,
        )?)
    }
    /// Select an explicit synthetic feed before sealing/admitting any session.
    pub fn remap_feed(&mut self, feed: FeedId) -> Result<()> {
        let mut data = self.data(1)?;
        data.delivery = self
            .providers
            .at(at(self.ms)?)
            .next()
            .ok_or("provider")?
            .value()
            .capability
            .declaration()
            .1;
        let session = self.seal(data)?;
        let routes = self
            .plan()?
            .streaming_routes(&session)?
            .into_iter()
            .map(|r| {
                RouteBinding::new(
                    RoutingKey::symbol(r.mapping().value().binding().0),
                    r.mapping().clone(),
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.context.scope.feed = feed;
        let interval = EffectiveInterval::new(at(self.ms - 1000)?, at(self.ms + 86_400_000)?)?;
        self.directories = ReferenceHistory::new(vec![version(
            501,
            1,
            self.ms - 1,
            RoutingDirectory::new(self.context.scope, interval, routes)?,
        )?])?;
        Ok(())
    }
    pub fn retain_first(&mut self, members: usize) -> Result<()> {
        self.scopes.truncate(members);
        let interval = EffectiveInterval::new(at(self.ms - 1000)?, at(self.ms + 86_400_000)?)?;
        let ids: Vec<_> = self.scopes.iter().map(|s| s.instrument).collect();
        self.selections = ReferenceHistory::new(vec![version(
            500,
            1,
            self.ms - 1,
            InstrumentSelection::new(interval, &ids)?,
        )?])?;
        Ok(())
    }
    pub fn session(&self, id: u128) -> Result<StreamingSession> {
        self.seal(self.data(id)?)
    }
    pub fn revoked(&self) -> Result<ReferenceHistory<RightsRecord>> {
        let mut versions: Vec<_> = self.rights.at(at(self.ms)?).cloned().collect();
        let additions = versions
            .iter()
            .map(|old| {
                let mut value = *old.value();
                value.status = ReviewStatus::Revoked;
                version(old.id().get(), 2, self.ms + 1, value)
            })
            .collect::<Result<Vec<_>>>()?;
        versions.extend(additions);
        Ok(ReferenceHistory::new(versions)?)
    }
}
