//! Canonical market observations, values and specifications; no transport or provider DTOs.
//! Constructors establish value invariants, not entitlement or evidence quality.
//! `Hash` supports collections, not stable evidence digests. Wire encoding and
//! content-addressed manifests belong to separately versioned contracts.
//!
//! ```
//! use edgeagent_market_data_domain::{UsdPrice, ValueError};
//! let price = UsdPrice::new(12345, 2)?;
//! assert_eq!(price.decimal_parts(), (12345, 2));
//! # Ok::<(), ValueError>(())
//! ```
//!
//! Identities cannot cross namespaces:
//! ```compile_fail,E0308
//! use edgeagent_market_data_domain::{InstrumentId, ListingId};
//! fn listing(_: ListingId) {}
//! fn invalid(id: InstrumentId) { listing(id); }
//! ```
//!
//! Safe callers cannot bypass numeric validation:
//! ```compile_fail,E0423
//! use edgeagent_market_data_domain::UsdPrice;
//! let invalid = UsdPrice(0);
//! ```
#![deny(missing_docs)]

mod actions;
mod calendar;
mod capability;
mod continuity;
mod daily;
mod date_only;
mod derivative;
mod evidence_time;
mod identity;
mod mapping;
mod market_value;
mod observation;
mod option_schedule;
mod quantity;
mod readiness;
mod reference;
mod time;

pub use actions::{ActionKind, CorporateAction};
pub use calendar::{CalendarDay, SessionHours, SessionWindow};
pub use capability::{
    BarCoverage, DeliveryCapability, EvidenceContract, EvidenceKind, KnowledgeCapability,
    MarketScope, ProviderCapability, SemanticReference, TimestampPrecision,
};
pub use continuity::{
    ContinuityError, ContinuityMode, ContinuityPolicy, ContinuityState, ContinuityTracker,
    FeedScope, IngestOutcome, RecoveryTicket,
};
pub use daily::{
    ActionCoverage, CheckedDailyDay, CheckedDailySeries, DailyQualityError, DailyQualityPolicy,
    DailyReferences, DailySeriesSpec,
};
pub use date_only::{DateOnlyBucket, DateOnlyTime};
pub use derivative::{
    DeliverableReference, DerivativeError, ExerciseStyle, OptionKind, OptionSpecification,
    OptionTerms, SettlementStyle, Underlying,
};
pub use evidence_time::{EvidenceInterval, EvidenceTimes, PreciseInstant};
pub use identity::{
    FeedEpoch, FeedId, InstrumentId, ListingId, ObservationId, ObservationRevision, ReferenceId,
    ReferenceRevision, SourceId, TransformationRevision,
};
pub use mapping::{DateRange, InstrumentMapping, Symbol, Venue};
pub use market_value::{ObservationValue, OptionQuoteSnapshot, QuoteSide, RawBar, ShareTrade};
pub use observation::{
    CanonicalObservation, ObservationContext, ObservationError, ObservationVersion,
};
pub use option_schedule::OptionSchedule;
pub use quantity::{
    ContractQuantity, Currency, DisplayedContracts, IndexPoints, OptionPoints, ShareVolume,
    UsdPerPoint, UsdPrice,
};
pub use readiness::{
    ContinuityRequirement, EvidenceInput, EvidenceRequirement, ReadinessPolicy, ReadyEvidence,
};
pub use reference::{EffectiveInterval, ReferenceError, ReferenceHistory, ReferenceVersion};
pub use time::{ObservationTimes, SessionDate, UtcInstant};

/// Payload-free failures from canonical value construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ValueError {
    /// A service-assigned identity or revision cannot be zero.
    ZeroIdentity,
    /// Prices must be positive; volumes must be nonnegative.
    InvalidSign,
    /// The normalized decimal coefficient exceeds 28 digits.
    Precision,
    /// Fractional scale exceeds the unit's supported precision.
    Scale,
    /// The Gregorian date is invalid or outside years 1 through 9999.
    Date,
    /// The UTC instant is outside years 1 through 9999.
    Instant,
    /// Observation timestamps contradict their causal order.
    TimeOrder,
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ZeroIdentity => "identity or revision must be nonzero",
            Self::InvalidSign => "invalid sign for market unit",
            Self::Precision => "decimal precision exceeds profile",
            Self::Scale => "decimal scale exceeds profile",
            Self::Date => "invalid session date",
            Self::Instant => "invalid UTC instant",
            Self::TimeOrder => "inconsistent observation times",
        })
    }
}

impl std::error::Error for ValueError {}
