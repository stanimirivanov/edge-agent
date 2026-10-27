//! Bounded local selection, not entitlement, source qualification or durable configuration.
use crate::{AcquisitionRequest, Cancellation, KnowledgeMode};
use edgeagent_market_data_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};

/// Payload-free selection/routing rejection; no provider data or aliases are attached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SelectionError {
    /// Bounded cardinality exceeded.
    Capacity,
    /// Missing, duplicate or conflicting reference/key binding.
    Mapping,
    /// Source, feed, epoch or request context differs.
    Scope,
    /// Required revision is unavailable at the cutoff or outside its effective interval.
    Revision,
    /// Explicit selection excludes the instrument (including an empty selection).
    NotSelected,
    /// Header sequence gap, duplicate, missing sequence or overflow.
    Continuity,
    /// Explicit reset invalidated this router.
    Reset,
    /// Directory update requires an atomic new plan, never incremental acceptance.
    DirectoryChanged,
    /// Authentication failure; no further data may be accepted.
    Authentication,
    /// Router was already invalidated; new data cannot heal it.
    Invalidated,
    /// Malformed or oversized frame, including unknown version/kind.
    Framing,
    /// Selected payload failed validation.
    Payload,
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instrument routing rejected: {self:?}")
    }
}
impl std::error::Error for SelectionError {}
type Result<T> = std::result::Result<T, SelectionError>;
macro_rules! redact { ($($t:ty),*) => {$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*}; }
mod encoding;
mod provider;
mod router;
pub use provider::SelectedDailyProvider;
pub use router::{EarlyRouter, FrameKind, FrameMetadata, RouteDecision};

/// One immutable explicit set; empty means stop all data, not subscribe to all.
#[derive(Clone, Eq, PartialEq)]
pub struct InstrumentSelection {
    effective: EffectiveInterval,
    members: BTreeSet<InstrumentId>,
}
impl InstrumentSelection {
    /// Bound selection to at most 32 distinct instruments (the existing P04 plan ceiling).
    /// # Errors
    /// Rejects duplicates and excess cardinality without silently truncating.
    pub fn new(effective: EffectiveInterval, members: &[InstrumentId]) -> Result<Self> {
        if members.len() > 32 {
            return Err(SelectionError::Capacity);
        }
        let unique: BTreeSet<_> = members.iter().copied().collect();
        if unique.len() != members.len() {
            return Err(SelectionError::Mapping);
        }
        Ok(Self {
            effective,
            members: unique,
        })
    }
}
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum Key {
    Symbol(String),
    Locate(NonZeroU32),
}
/// Checked alias or source-local locate. Its meaning is scoped by feed and epoch.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct RoutingKey(Key);
impl RoutingKey {
    /// Preserve a checked case-sensitive alias; no normalization or ticker identity inference.
    #[must_use]
    pub fn symbol(symbol: &Symbol) -> Self {
        Self(Key::Symbol(symbol.as_str().to_owned()))
    }
    /// Explicit nonzero locate in the supplied feed directory, not a native ITCH implementation.
    #[must_use]
    pub const fn locate(locate: NonZeroU32) -> Self {
        Self(Key::Locate(locate))
    }
    /// Explicit symbol access for protocol encoding; diagnostics redact it.
    #[must_use]
    pub fn as_symbol(&self) -> Option<&str> {
        match &self.0 {
            Key::Symbol(s) => Some(s),
            Key::Locate(_) => None,
        }
    }
    /// Explicit locate access, without expanding its feed/epoch namespace.
    #[must_use]
    pub const fn as_locate(&self) -> Option<NonZeroU32> {
        match self.0 {
            Key::Locate(n) => Some(n),
            Key::Symbol(_) => None,
        }
    }
}
/// Directory entry reusing an existing versioned canonical instrument/listing mapping.
#[derive(Clone, Eq, PartialEq)]
pub struct RouteBinding {
    key: RoutingKey,
    mapping: ReferenceVersion<InstrumentMapping>,
}
impl RouteBinding {
    /// A symbol key must exactly match the mapping; locate authenticity remains owner-verified.
    /// # Errors
    /// Rejects aliases that contradict their canonical mapping.
    pub fn new(key: RoutingKey, mapping: ReferenceVersion<InstrumentMapping>) -> Result<Self> {
        if key
            .as_symbol()
            .is_some_and(|s| s != mapping.value().binding().0.as_str())
        {
            return Err(SelectionError::Mapping);
        }
        Ok(Self { key, mapping })
    }
}
/// Exact scope for one atomic directory and header cursor; no account credentials.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RoutingScope {
    /// Canonical provenance source.
    pub source: SourceId,
    /// Qualified feed namespace; identical locates in another feed are unrelated.
    pub feed: FeedId,
    /// Feed incarnation; resets require a new qualified epoch.
    pub epoch: FeedEpoch,
}
/// Complete immutable directory snapshot; changes replace it atomically through reference history.
#[derive(Clone, Eq, PartialEq)]
pub struct RoutingDirectory {
    scope: RoutingScope,
    effective: EffectiveInterval,
    entries: BTreeMap<RoutingKey, ReferenceVersion<InstrumentMapping>>,
}
impl RoutingDirectory {
    /// Bound one directory to 4096 keys. Two keys may name the same exact mapping.
    /// # Errors
    /// Rejects duplicate keys, conflicting instrument mappings and foreign sources.
    pub fn new(
        scope: RoutingScope,
        effective: EffectiveInterval,
        entries: Vec<RouteBinding>,
    ) -> Result<Self> {
        if entries.len() > 4096 {
            return Err(SelectionError::Capacity);
        }
        let mut by_instrument = BTreeMap::new();
        let mut map = BTreeMap::new();
        for RouteBinding { key, mapping } in entries {
            if mapping.source() != scope.source {
                return Err(SelectionError::Scope);
            }
            let instrument = mapping.value().identities().0;
            if by_instrument
                .insert(instrument, mapping.clone())
                .is_some_and(|old| old != mapping)
                || map.insert(key, mapping).is_some()
            {
                return Err(SelectionError::Mapping);
            }
        }
        Ok(Self {
            scope,
            effective,
            entries: map,
        })
    }
}
/// Explicit effective/knowledge context. Receipt cutoffs never come from a provider Date header.
#[derive(Clone, Copy)]
pub struct SelectionContext {
    /// Exact directory namespace.
    pub scope: RoutingScope,
    /// Effective instant for selection/directory validity.
    pub effective_at: UtcInstant,
    /// Inclusive local knowledge cutoff.
    pub cutoff: UtcInstant,
    /// Explicit mapping session; not inferred from a host timezone.
    pub session: SessionDate,
}
/// Immutable resolved revisions; not a retained read grant or current-rights credential.
pub struct SelectionPlan<'a> {
    context: SelectionContext,
    selection: &'a ReferenceVersion<InstrumentSelection>,
    directory: &'a ReferenceVersion<RoutingDirectory>,
}
/// Resolved checked mapping, available only after selection/context validation.
#[derive(Clone, Copy)]
pub struct InstrumentRoute<'a>(&'a ReferenceVersion<InstrumentMapping>);
/// Exact narrowed request and frozen selection configuration, not a rights grant.
pub struct SelectedRequest<'a> {
    request: &'a AcquisitionRequest,
    route: InstrumentRoute<'a>,
    plan: &'a SelectionPlan<'a>,
}
impl SelectedRequest<'_> {
    /// Original immutable request, with no widened universe or rewritten symbol identity.
    #[must_use]
    pub const fn request(&self) -> &AcquisitionRequest {
        self.request
    }
    /// Exact resolved alias/listing witness for outbound provider encoding.
    #[must_use]
    pub const fn route(&self) -> InstrumentRoute<'_> {
        self.route
    }
    /// Canonical selection/directory configuration to bind into replay manifests.
    /// # Errors
    /// Rejects unsupported future encodings rather than generating unstable fingerprints.
    pub fn binding_bytes(&self) -> Result<Vec<u8>> {
        self.plan.binding_bytes()
    }
}
impl InstrumentRoute<'_> {
    /// Exact mapping witness, including alias/listing, dates and source revision.
    #[must_use]
    pub const fn mapping(&self) -> &ReferenceVersion<InstrumentMapping> {
        self.0
    }
}
impl<'a> SelectionPlan<'a> {
    /// Exact feed/epoch namespace of this frozen directory; not source continuity evidence.
    #[must_use]
    pub const fn routing_scope(&self) -> RoutingScope {
        self.context.scope
    }
    /// Resolve selected symbol mappings only when this complete plan matches the sealed session.
    /// # Errors
    /// Rejects a different selection/directory witness, missing aliases or ambiguous instruments.
    pub fn streaming_routes(
        &self,
        session: &crate::StreamingSession,
    ) -> Result<Vec<InstrumentRoute<'_>>> {
        #[derive(serde::Deserialize)]
        struct Witness {
            selection: Vec<u8>,
        }
        let witness: Witness =
            serde_json::from_slice(session.bytes()).map_err(|_| SelectionError::Scope)?;
        if witness.selection != self.binding_bytes()? {
            return Err(SelectionError::Scope);
        }
        session
            .data()
            .scopes
            .iter()
            .map(|scope| {
                let mut mappings = self
                    .directory
                    .value()
                    .entries
                    .iter()
                    .filter(|(_, mapping)| mapping.value().identities().0 == scope.instrument);
                let (key, mapping) = mappings.next().ok_or(SelectionError::Mapping)?;
                if mappings.next().is_some() || key.as_symbol().is_none() {
                    return Err(SelectionError::Mapping);
                }
                Ok(InstrumentRoute(mapping))
            })
            .collect()
    }
    pub(crate) fn streaming_binding(
        &self,
        scopes: &[crate::AccessScope],
        window: EffectiveInterval,
        at: UtcInstant,
    ) -> Result<Vec<u8>> {
        let members: BTreeSet<_> = scopes.iter().map(|s| s.instrument).collect();
        if members != self.selection.value().members
            || scopes.iter().any(|s| s.source != self.context.scope.source)
            || self.context.effective_at != window.bounds().0
            || self.context.cutoff > at
            || self.selection.value().effective.bounds().1 < window.bounds().1
            || self.directory.value().effective.bounds().1 < window.bounds().1
        {
            return Err(SelectionError::Scope);
        }
        self.binding_bytes()
    }
    /// Choose each requested record's latest known revision BEFORE checking scope/effectivity.
    /// # Errors
    /// Rejects missing/foreign/inactive revisions, future mapping receipts, ambiguous mappings
    /// and any selected instrument absent from the complete directory.
    pub fn resolve(
        context: SelectionContext,
        ids: (ReferenceId, ReferenceId),
        selections: &'a ReferenceHistory<InstrumentSelection>,
        directories: &'a ReferenceHistory<RoutingDirectory>,
    ) -> Result<Self> {
        let selection = selections
            .at(context.cutoff)
            .find(|v| v.id() == ids.0)
            .ok_or(SelectionError::Revision)?;
        let directory = directories
            .at(context.cutoff)
            .find(|v| v.id() == ids.1)
            .ok_or(SelectionError::Revision)?;
        if selection.source() != context.scope.source
            || directory.source() != context.scope.source
            || directory.value().scope != context.scope
        {
            return Err(SelectionError::Scope);
        }
        if !selection.value().effective.contains(context.effective_at)
            || !directory.value().effective.contains(context.effective_at)
        {
            return Err(SelectionError::Revision);
        }
        for mapping in directory.value().entries.values() {
            if mapping.known_at() > directory.known_at()
                || !mapping.value().binding().2.contains(context.session)
            {
                return Err(SelectionError::Mapping);
            }
        }
        if selection.value().members.iter().any(|id| {
            !directory
                .value()
                .entries
                .values()
                .any(|m| m.value().identities().0 == *id)
        }) {
            return Err(SelectionError::Mapping);
        }
        Ok(Self {
            context,
            selection,
            directory,
        })
    }
    /// Resolve one daily request BEFORE provider invocation or call reservation.
    /// # Errors
    /// Rejects foreign source/context, excluded instruments and mappings that do not cover
    /// the entire request range. Split genuine alias changes explicitly; never guess an alias.
    pub fn prepare<'p>(&'p self, request: &'p AcquisitionRequest) -> Result<SelectedRequest<'p>> {
        request
            .validate(request.admitted_at, &Cancellation::default())
            .map_err(|_| SelectionError::Scope)?;
        let cutoff = match request.knowledge {
            KnowledgeMode::Capture => request.admitted_at,
            KnowledgeMode::RetainedAt(at) => at,
        };
        if request.scope.source != self.context.scope.source
            || request.admitted_at != self.context.effective_at
            || cutoff != self.context.cutoff
        {
            return Err(SelectionError::Scope);
        }
        if !self
            .selection
            .value()
            .members
            .contains(&request.scope.instrument)
        {
            return Err(SelectionError::NotSelected);
        }
        let mapping = self
            .directory
            .value()
            .entries
            .values()
            .find(|m| m.value().identities().0 == request.scope.instrument)
            .ok_or(SelectionError::Mapping)?;
        let (start, end) = mapping.value().binding().2.bounds();
        let (from, to) = request.range.bounds();
        if start > from || end < to {
            return Err(SelectionError::Mapping);
        }
        Ok(SelectedRequest {
            request,
            route: InstrumentRoute(mapping),
            plan: self,
        })
    }
    /// Canonical bounded private configuration bytes, including actual values and witnesses.
    /// Hash these into the existing P05 configuration digest; not authentication or public wire.
    /// # Errors
    /// Rejects unsupported future venue encodings rather than assigning unstable discriminants.
    pub fn binding_bytes(&self) -> Result<Vec<u8>> {
        encoding::binding(self)
    }
}
redact!(
    InstrumentSelection,
    RoutingKey,
    RouteBinding,
    RoutingScope,
    RoutingDirectory,
    SelectionContext,
    SelectionPlan<'_>,
    InstrumentRoute<'_>,
    SelectedRequest<'_>
);
