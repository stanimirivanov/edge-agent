use crate::{
    BarCoverage, DisplayedContracts, EffectiveInterval, EvidenceInterval, EvidenceKind,
    EvidenceTimes, IndexPoints, InstrumentId, ObservationError, OptionPoints, OptionSpecification,
    PreciseInstant, ReferenceId, ReferenceRevision, ReferenceVersion, ShareVolume, UsdPrice,
    UtcInstant,
};

/// Raw, unadjusted OHLCV. It never claims a regular session from duration alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawBar {
    interval: EvidenceInterval,
    coverage: BarCoverage,
    ohlc: [UsdPrice; 4],
    volume: ShareVolume,
}
impl RawBar {
    /// Validate open/high/low/close and an explicit half-open bucket interval.
    /// Daily provider buckets and core calendar sessions remain distinct.
    ///
    /// # Errors
    /// Returns `Ohlc` unless low <= open/close <= high. Zero volume is valid.
    pub fn new(
        interval: EvidenceInterval,
        coverage: BarCoverage,
        ohlc: [UsdPrice; 4],
        volume: ShareVolume,
    ) -> Result<Self, ObservationError> {
        let [open, high, low, close] = ohlc;
        if low > high || open < low || open > high || close < low || close > high {
            return Err(ObservationError::Ohlc);
        }
        Ok(Self {
            interval,
            coverage,
            ohlc,
            volume,
        })
    }
    /// Inclusive bucket start/exclusive end and its explicitly declared coverage.
    #[must_use]
    pub const fn bucket(self) -> (EvidenceInterval, BarCoverage) {
        (self.interval, self.coverage)
    }
    /// Open, high, low, close in that order, and reported share volume.
    #[must_use]
    pub const fn values(self) -> ([UsdPrice; 4], ShareVolume) {
        (self.ohlc, self.volume)
    }
}
/// Underlying trade; never an index level or inferred option execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShareTrade {
    price: UsdPrice,
    shares: ShareVolume,
}
impl ShareTrade {
    /// Construct a positive-price, positive-size trade.
    ///
    /// # Errors
    /// Returns `TradeSize` for zero reported shares.
    pub fn new(price: UsdPrice, shares: ShareVolume) -> Result<Self, ObservationError> {
        if shares.decimal_parts().0 == 0 {
            return Err(ObservationError::TradeSize);
        }
        Ok(Self { price, shares })
    }
    /// Exact USD/share and positive reported shares.
    #[must_use]
    pub const fn values(self) -> (UsdPrice, ShareVolume) {
        (self.price, self.shares)
    }
}
/// One option side. Missing size, known zero and positive display are distinct;
/// none establishes executable liquidity or queue position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuoteSide {
    points: OptionPoints,
    size: Option<DisplayedContracts>,
    at: PreciseInstant,
}
impl QuoteSide {
    /// Preserve exact premium points, optional whole displayed contracts and side time.
    #[must_use]
    pub const fn new(
        points: OptionPoints,
        size: Option<DisplayedContracts>,
        at: PreciseInstant,
    ) -> Self {
        Self { points, size, at }
    }
    /// Explicit premium/size access; diagnostics redact both.
    #[must_use]
    pub const fn values(self) -> (OptionPoints, Option<DisplayedContracts>) {
        (self.points, self.size)
    }
    /// Source instant for this side, independently retained.
    #[must_use]
    pub const fn at(self) -> PreciseInstant {
        self.at
    }
}
/// Two-sided premium snapshot tied to an exact specification version. Structural
/// validation does not enable any exercise/settlement profile or certify freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionQuoteSnapshot {
    instrument: InstrumentId,
    effective: EffectiveInterval,
    reference: (ReferenceId, ReferenceRevision),
    known_at: UtcInstant,
    bid: QuoteSide,
    ask: QuoteSide,
}
impl OptionQuoteSnapshot {
    /// Pin supplied reference and reject crossed/off-tick prices. Locked and zero
    /// quotes remain representable; later quality/fill policy decides usability.
    ///
    /// # Errors
    /// Returns `CrossedQuote` for bid > ask or `Tick` for either off-tick side.
    pub fn new(
        spec: &ReferenceVersion<OptionSpecification>,
        bid: QuoteSide,
        ask: QuoteSide,
    ) -> Result<Self, ObservationError> {
        if bid.points > ask.points {
            return Err(ObservationError::CrossedQuote);
        }
        let (tick, _, _) = spec.value().terms().quotation();
        if !bid.points.is_multiple_of(tick) || !ask.points.is_multiple_of(tick) {
            return Err(ObservationError::Tick);
        }
        Ok(Self {
            instrument: spec.value().id(),
            effective: spec.value().effective(),
            reference: (spec.id(), spec.revision()),
            known_at: spec.known_at(),
            bid,
            ask,
        })
    }
    /// Bid and ask, never synthesized from a trade or theoretical price.
    #[must_use]
    pub const fn sides(self) -> (QuoteSide, QuoteSide) {
        (self.bid, self.ask)
    }
    /// Exact specification reference; its full history/authority remains externally verified.
    #[must_use]
    pub const fn specification(self) -> (ReferenceId, ReferenceRevision) {
        self.reference
    }
    pub(crate) fn validate(
        self,
        instrument: crate::InstrumentId,
        times: EvidenceTimes,
        precision: crate::TimestampPrecision,
    ) -> Result<(), ObservationError> {
        if self.instrument != instrument
            || PreciseInstant::from_millis(self.known_at) > times.received_at()
        {
            return Err(ObservationError::Specification);
        }
        for side in [self.bid, self.ask] {
            if side.at > times.event_at() {
                return Err(ObservationError::TimeOrder);
            }
            if !precision.accepts(side.at) {
                return Err(ObservationError::Timestamp);
            }
            if !self.effective.contains(side.at.millisecond_floor()) {
                return Err(ObservationError::Specification);
            }
        }
        if !self
            .effective
            .contains(times.event_at().millisecond_floor())
        {
            return Err(ObservationError::Specification);
        }
        Ok(())
    }
}

/// Canonical payload alternatives. A cancellation has no fabricated numeric payload.
///
/// ```compile_fail,E0308
/// use edgeagent_market_data_domain::{IndexPoints, ObservationValue, UsdPrice};
/// fn invalid(price: UsdPrice) { let _ = ObservationValue::IndexLevel(price); }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservationValue {
    /// Unadjusted USD/share aggregate.
    Bar(RawBar),
    /// Underlying execution report in shares.
    Trade(ShareTrade),
    /// Non-tradable cash-index level; no implicit ETF/future proxy.
    IndexLevel(IndexPoints),
    /// Option premium snapshot with independent sides.
    OptionQuote(OptionQuoteSnapshot),
}
impl ObservationValue {
    pub(crate) const fn kind(self) -> EvidenceKind {
        match self {
            Self::Bar(bar) => EvidenceKind::RawBar(bar.coverage),
            Self::Trade(_) => EvidenceKind::ShareTrade,
            Self::IndexLevel(_) => EvidenceKind::IndexLevel,
            Self::OptionQuote(_) => EvidenceKind::OptionSnapshot,
        }
    }
}
