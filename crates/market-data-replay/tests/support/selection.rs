use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::num::NonZeroU32;
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn time(ms: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
pub fn version<T>(
    id: u128,
    rev: u64,
    known: UtcInstant,
    source: SourceId,
    value: T,
) -> Result<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(id)?,
        ReferenceRevision::new(rev)?,
        source,
        known,
        [7; 32],
        value,
    ))
}
pub struct SelectionFixture {
    pub context: SelectionContext,
    pub selections: ReferenceHistory<InstrumentSelection>,
    pub directories: ReferenceHistory<RoutingDirectory>,
}
impl SelectionFixture {
    pub fn new(request: &AcquisitionRequest, members: &[InstrumentId]) -> Result<Self> {
        Self::with_alias(request, members, "INVENTED-A")
    }
    pub fn with_alias(
        request: &AcquisitionRequest,
        members: &[InstrumentId],
        alias: &str,
    ) -> Result<Self> {
        let context = SelectionContext {
            scope: RoutingScope {
                source: request.scope.source,
                feed: FeedId::new(1)?,
                epoch: FeedEpoch::new(1)?,
            },
            effective_at: request.admitted_at,
            cutoff: request.admitted_at,
            session: request.range.bounds().0,
        };
        let effective = EffectiveInterval::new(
            time(0)?,
            time(request.admitted_at.unix_millis() + 86_400_000)?,
        )?;
        let selections = ReferenceHistory::new(vec![version(
            1,
            1,
            time(0)?,
            context.scope.source,
            InstrumentSelection::new(effective, members)?,
        )?])?;
        let mut entries = Vec::new();
        for (id, name) in [(1, alias), (2, "INVENTED-B")] {
            let mapping = version(
                id,
                1,
                time(0)?,
                context.scope.source,
                InstrumentMapping::new(
                    InstrumentId::new(id)?,
                    ListingId::new(id)?,
                    Symbol::new(name)?,
                    Venue::Xnys,
                    request.range,
                ),
            )?;
            entries.push(RouteBinding::new(
                RoutingKey::symbol(mapping.value().binding().0),
                mapping.clone(),
            )?);
            entries.push(RouteBinding::new(
                RoutingKey::locate(NonZeroU32::new(id as u32).ok_or("locate")?),
                mapping,
            )?);
        }
        let directories = ReferenceHistory::new(vec![version(
            2,
            1,
            time(0)?,
            context.scope.source,
            RoutingDirectory::new(context.scope, effective, entries)?,
        )?])?;
        Ok(Self {
            context,
            selections,
            directories,
        })
    }
    pub fn plan(&self) -> Result<SelectionPlan<'_>> {
        Ok(SelectionPlan::resolve(
            self.context,
            (ReferenceId::new(1)?, ReferenceId::new(2)?),
            &self.selections,
            &self.directories,
        )?)
    }
    pub fn header(&self, sequence: Option<u64>, kind: FrameKind) -> FrameMetadata {
        FrameMetadata {
            scope: self.context.scope,
            sequence,
            kind,
        }
    }
}
pub fn symbol(name: &str) -> Result<RoutingKey> {
    Ok(RoutingKey::symbol(&Symbol::new(name)?))
}
