use crate::{ObservationError, PreciseInstant, ReferenceId, ReferenceRevision, SourceId, Venue};

/// Exact version of reviewed source/feed semantics, including condition-code,
/// correction, session and trade-eligibility rules. A reference is not proof of
/// coverage or rights; consumers must resolve and qualify it before use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticReference(ReferenceId, ReferenceRevision);
impl SemanticReference {
    /// Pin an exact checked catalog identity/revision, never an unversioned alias.
    #[must_use]
    pub const fn new(id: ReferenceId, revision: ReferenceRevision) -> Self {
        Self(id, revision)
    }
    /// Return the exact catalog key.
    #[must_use]
    pub const fn version(self) -> (ReferenceId, ReferenceRevision) {
        (self.0, self.1)
    }
}

/// Price discovery scope; a consolidated feed and a venue feed are not substitutes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarketScope {
    /// An explicitly supported equity listing venue.
    Venue(Venue),
    /// Consolidated underlying or option quotations/trades; details are pinned by semantics.
    Consolidated,
    /// Non-tradable cash-index observations from the identified publisher.
    IndexPublisher,
}
/// Bar aggregation coverage, never inferred from its interval duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BarCoverage {
    /// Raw provider-day aggregation, not regular-session-only.
    ProviderDay,
    /// Explicit core-session aggregation under qualified calendar/eligibility rules.
    CoreSession,
    /// Other explicit intraday coverage under the pinned semantics.
    Intraday,
}
/// Canonical evidence class; index levels cannot masquerade as trades.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EvidenceKind {
    /// Raw, unadjusted USD/share OHLCV with explicit coverage.
    RawBar(BarCoverage),
    /// Positive-size underlying trade in USD/share.
    ShareTrade,
    /// Non-tradable positive index points.
    IndexLevel,
    /// Two-sided option premium snapshot, not depth or queue position.
    OptionSnapshot,
}
/// Declared source timestamp granularity. Local receipt may be more precise.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TimestampPrecision {
    /// Exact whole milliseconds.
    Milliseconds,
    /// Exact whole microseconds.
    Microseconds,
    /// Exact nanoseconds.
    Nanoseconds,
}
impl TimestampPrecision {
    pub(crate) fn accepts(self, at: PreciseInstant) -> bool {
        match self {
            Self::Milliseconds => at.submillis() == 0,
            Self::Microseconds => at.submillis().is_multiple_of(1000),
            Self::Nanoseconds => true,
        }
    }
}

/// Exact evidence semantics requested by a consumer or declared by a source.
/// No wildcard, implicit index proxy, adjustment or scope conversion is supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContract {
    kind: EvidenceKind,
    scope: MarketScope,
    semantics: SemanticReference,
    precision: TimestampPrecision,
}
impl EvidenceContract {
    /// Bind an explicit evidence kind, discovery scope, semantic revision and precision.
    ///
    /// # Errors
    /// Returns `Contract` if index/non-index scope contradicts the kind, or a
    /// stock listing venue is presented as an options quotation venue.
    pub fn new(
        kind: EvidenceKind,
        scope: MarketScope,
        semantics: SemanticReference,
        precision: TimestampPrecision,
    ) -> Result<Self, ObservationError> {
        if (kind == EvidenceKind::IndexLevel) != (scope == MarketScope::IndexPublisher)
            || (kind == EvidenceKind::OptionSnapshot && matches!(scope, MarketScope::Venue(_)))
        {
            return Err(ObservationError::Contract);
        }
        Ok(Self {
            kind,
            scope,
            semantics,
            precision,
        })
    }
    /// Canonical kind and discovery scope, without changing either's meaning.
    #[must_use]
    pub const fn shape(self) -> (EvidenceKind, MarketScope) {
        (self.kind, self.scope)
    }
    /// Exact semantic reference and declared source precision.
    #[must_use]
    pub const fn rules(self) -> (SemanticReference, TimestampPrecision) {
        (self.semantics, self.precision)
    }
}
/// Delivery promise, distinct from actual continuity/readiness established in P04.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DeliveryCapability {
    /// Independent snapshots; absent sequence is not proof of missing messages.
    Snapshots,
    /// Source sequence within the stream scope defined by the semantic reference.
    Sequenced,
}
/// Historical knowledge support, not simply the ability to fetch old event dates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KnowledgeCapability {
    /// Current capture only; cannot establish earlier knowledge.
    CaptureOnly,
    /// Qualified retained versions/availability history, still checked per request.
    HistoricalVersions,
}
/// One finite source capability; declare multiple values for multiple contracts.
/// This descriptor performs no I/O and grants no entitlement or automatic failover.
///
/// ```
/// use edgeagent_market_data_domain::*;
/// let rules = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
/// let contract = EvidenceContract::new(EvidenceKind::IndexLevel,
///     MarketScope::IndexPublisher, rules, TimestampPrecision::Nanoseconds)?;
/// let source = ProviderCapability::new(SourceId::new(1)?, contract,
///     DeliveryCapability::Snapshots, KnowledgeCapability::CaptureOnly);
/// assert_eq!(source.require(contract, DeliveryCapability::Sequenced,
///     KnowledgeCapability::CaptureOnly), Err(ObservationError::UnsupportedCapability));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCapability {
    source: SourceId,
    contract: EvidenceContract,
    delivery: DeliveryCapability,
    knowledge: KnowledgeCapability,
}
impl ProviderCapability {
    /// Record a declaration; its accuracy, account limits and rights require qualification.
    #[must_use]
    pub const fn new(
        source: SourceId,
        contract: EvidenceContract,
        delivery: DeliveryCapability,
        knowledge: KnowledgeCapability,
    ) -> Self {
        Self {
            source,
            contract,
            delivery,
            knowledge,
        }
    }
    /// Source identity remains visible even when two providers meet one contract.
    #[must_use]
    pub const fn source(self) -> SourceId {
        self.source
    }
    /// Declared contract, delivery and historical-knowledge support.
    #[must_use]
    pub const fn declaration(self) -> (EvidenceContract, DeliveryCapability, KnowledgeCapability) {
        (self.contract, self.delivery, self.knowledge)
    }
    /// Check exact evidence semantics plus required delivery/knowledge guarantees.
    /// A sequenced source may serve snapshots; historical versions may serve capture.
    /// Finer timestamps are not silently relabeled as a coarser source precision.
    ///
    /// # Errors
    /// Returns `UnsupportedCapability` on any mismatch, with no permissive defaults.
    pub fn require(
        self,
        contract: EvidenceContract,
        delivery: DeliveryCapability,
        knowledge: KnowledgeCapability,
    ) -> Result<(), ObservationError> {
        if self.contract != contract
            || (delivery == DeliveryCapability::Sequenced && self.delivery != delivery)
            || (knowledge == KnowledgeCapability::HistoricalVersions && self.knowledge != knowledge)
        {
            return Err(ObservationError::UnsupportedCapability);
        }
        Ok(())
    }
}
