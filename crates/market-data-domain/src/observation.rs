use crate::{
    DeliveryCapability, EvidenceTimes, InstrumentId, ObservationId, ObservationRevision,
    ObservationValue, ProviderCapability, SessionDate, TransformationRevision,
};

/// Payload-free canonical evidence failures, safe for public diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservationError {
    /// Invalid/out-of-range timestamp or precision mismatch; no truncation occurs.
    Timestamp,
    /// Source/provider/receipt/side timestamps contradict causal order.
    TimeOrder,
    /// Impossible OHLC ordering.
    Ohlc,
    /// Zero-sized underlying trade.
    TradeSize,
    /// Bid exceeds ask.
    CrossedQuote,
    /// Option premium is off the specification's fixed tick grid.
    Tick,
    /// Wrong, ineffective or not-yet-known option specification.
    Specification,
    /// Evidence kind, market scope or payload contradicts its contract.
    Contract,
    /// Source capability does not satisfy an explicit requirement.
    UnsupportedCapability,
    /// Revision/predecessor or cancellation metadata is inconsistent.
    Revision,
    /// Required source sequence is absent; presence alone proves no continuity.
    Sequence,
    /// More than 16 record-level condition codes, or duplicates/unsorted codes.
    Conditions,
}
impl std::fmt::Display for ObservationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Timestamp => "invalid evidence timestamp or precision",
            Self::TimeOrder => "inconsistent evidence time order",
            Self::Ohlc => "invalid OHLC relationship",
            Self::TradeSize => "trade size must be positive",
            Self::CrossedQuote => "crossed option quote",
            Self::Tick => "option quote violates tick grid",
            Self::Specification => "unavailable or conflicting option specification",
            Self::Contract => "inconsistent evidence contract",
            Self::UnsupportedCapability => "unsupported evidence capability",
            Self::Revision => "inconsistent observation revision",
            Self::Sequence => "source sequence required",
            Self::Conditions => "invalid bounded condition codes",
        })
    }
}
impl std::error::Error for ObservationError {}

/// One logical source observation's service-owned revision. This metadata does
/// not deduplicate records or verify that its predecessor actually exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationVersion {
    id: ObservationId,
    revision: ObservationRevision,
    replaces: Option<ObservationRevision>,
}
impl ObservationVersion {
    /// Revision 1 has no predecessor. Later revisions name an earlier one;
    /// a partial capture may skip revisions but never invent an original.
    ///
    /// # Errors
    /// Returns `Revision` for missing, forward, self or unexpected predecessors.
    pub fn new(
        id: ObservationId,
        revision: ObservationRevision,
        replaces: Option<ObservationRevision>,
    ) -> Result<Self, ObservationError> {
        if match replaces {
            None => revision.get() != 1,
            Some(previous) => previous >= revision,
        } {
            return Err(ObservationError::Revision);
        }
        Ok(Self {
            id,
            revision,
            replaces,
        })
    }
    /// Logical identity, current revision and optional replaced revision.
    #[must_use]
    pub const fn lineage(
        self,
    ) -> (
        ObservationId,
        ObservationRevision,
        Option<ObservationRevision>,
    ) {
        (self.id, self.revision, self.replaces)
    }
}
/// Instrument, source semantics, transformation, local session label and bounded
/// record-level condition codes. No field grants data rights or proves a session.
#[derive(Clone, Eq, PartialEq)]
pub struct ObservationContext {
    instrument: InstrumentId,
    provider: ProviderCapability,
    transformation: TransformationRevision,
    session: SessionDate,
    conditions: Vec<u16>,
}
impl ObservationContext {
    /// Preserve canonical record-level codes under the pinned semantic catalog.
    /// Adapters must map/qualify source codes; sorted unique input is required,
    /// never silently normalized. Side-specific conditions need a future profile.
    ///
    /// # Errors
    /// Returns `Conditions` before allocation for >16, duplicate or unsorted codes.
    pub fn new(
        instrument: InstrumentId,
        provider: ProviderCapability,
        transformation: TransformationRevision,
        session: SessionDate,
        conditions: &[u16],
    ) -> Result<Self, ObservationError> {
        if conditions.len() > 16 || conditions.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
            return Err(ObservationError::Conditions);
        }
        Ok(Self {
            instrument,
            provider,
            transformation,
            session,
            conditions: conditions.to_vec(),
        })
    }
    /// Stable instrument, declared source capability and transformation revision.
    #[must_use]
    pub const fn origin(&self) -> (InstrumentId, ProviderCapability, TransformationRevision) {
        (self.instrument, self.provider, self.transformation)
    }
    /// Supplied exchange-local label and record-level codes; neither is inferred.
    #[must_use]
    pub fn session_conditions(&self) -> (SessionDate, &[u16]) {
        (self.session, &self.conditions)
    }
}
impl std::fmt::Debug for ObservationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObservationContext(..)")
    }
}

/// Immutable canonical source revision. `None` means an explicit cancellation,
/// not missing data or a zero-valued observation. No history/freshness/rights
/// guarantee is inferred; P04 and application acceptance own those policies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalObservation {
    context: ObservationContext,
    version: ObservationVersion,
    times: EvidenceTimes,
    sequence: Option<u64>,
    value: Option<ObservationValue>,
}
impl CanonicalObservation {
    /// Validate payload/contract, source precision, sequence and cancellation.
    /// Bars preserve a source bucket label between its start and end (inclusive);
    /// actual receipt cannot precede bucket end. A start label is not availability.
    ///
    /// # Errors
    /// Returns the specific `ObservationError` for each violated boundary.
    /// Corrections do not overwrite previous observations; callers retain versions.
    pub fn new(
        context: ObservationContext,
        version: ObservationVersion,
        times: EvidenceTimes,
        sequence: Option<u64>,
        value: Option<ObservationValue>,
    ) -> Result<Self, ObservationError> {
        let (contract, delivery, _) = context.provider.declaration();
        let (_, precision) = contract.rules();
        let (observed, published) = times.provider_times();
        for at in [Some(times.event_at()), observed, published]
            .into_iter()
            .flatten()
        {
            if !precision.accepts(at) {
                return Err(ObservationError::Timestamp);
            }
        }
        if delivery == DeliveryCapability::Sequenced && sequence.is_none() {
            return Err(ObservationError::Sequence);
        }
        match value {
            None if version.replaces.is_none() => return Err(ObservationError::Revision),
            Some(v) if v.kind() != contract.shape().0 => return Err(ObservationError::Contract),
            Some(ObservationValue::Bar(bar)) => {
                let (start, end) = bar.bucket().0.bounds();
                if !precision.accepts(start) || !precision.accepts(end) {
                    return Err(ObservationError::Timestamp);
                }
                if times.event_at() < start || times.event_at() > end || times.received_at() < end {
                    return Err(ObservationError::TimeOrder);
                }
            }
            Some(ObservationValue::OptionQuote(quote)) => {
                quote.validate(context.instrument, times, precision)?;
            }
            _ => {}
        }
        Ok(Self {
            context,
            version,
            times,
            sequence,
            value,
        })
    }
    /// Explicit provenance/context access; diagnostics redact condition codes.
    #[must_use]
    pub const fn context(&self) -> &ObservationContext {
        &self.context
    }
    /// Stable revision metadata; equality of keys alone is not content equality.
    #[must_use]
    pub const fn version(&self) -> ObservationVersion {
        self.version
    }
    /// Source/provider/receipt times without invented historical knowledge.
    #[must_use]
    pub const fn times(&self) -> EvidenceTimes {
        self.times
    }
    /// Optional sequence; zero is valid and scope comes from the semantic reference.
    #[must_use]
    pub const fn sequence(&self) -> Option<u64> {
        self.sequence
    }
    /// Preserved payload or explicit cancellation. Never use cancellation as a price.
    #[must_use]
    pub const fn value(&self) -> Option<ObservationValue> {
        self.value
    }
}
