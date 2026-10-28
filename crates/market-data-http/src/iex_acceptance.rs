//! Explicit owner-selected interpretation, not a live source or self-authenticating grant.
mod accept;
mod companion;
mod replay;
pub use replay::IexReplayContext;
mod encoding;
mod inputs;
#[cfg(test)]
mod tests;
use crate::IexObservation;
use edgeagent_contracts::evidence::{DailyEvidenceArtifact, EvidenceDigest};
use edgeagent_market_data_domain::*;
use std::num::NonZeroU64;

/// Payload-free acceptance failure; never log source codes, symbols or native values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IexAcceptanceError {
    /// Invalid, ambiguous or excessive owner profile input.
    Configuration,
    /// Profile commitment, admitted selection or provider contract differs.
    Binding,
    /// Exact current calendar/mapping reference is absent, changed or unavailable at receipt.
    Reference,
    /// Current capture rights, receipt window, clock or cancellation rejected the operation.
    Authority,
    /// No explicit exact-set condition rule or identity lane matches.
    Semantics,
    /// Native value, time, minute bucket or coordinates are inconsistent.
    Observation,
}
impl std::fmt::Display for IexAcceptanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IEX acceptance rejected: {self:?}")
    }
}
impl std::error::Error for IexAcceptanceError {}
type Result<T> = std::result::Result<T, IexAcceptanceError>;

/// One reviewed exact source-condition set and its sorted unique canonical codes.
/// Empty input is permitted only as an explicit rule, never as inferred eligibility.
#[derive(Clone)]
pub struct IexConditionRule {
    raw: Vec<String>,
    canonical: Vec<u16>,
}
impl IexConditionRule {
    /// Accept at most 16 sorted unique source codes (1..=8 graphic ASCII bytes each)
    /// and at most 16 sorted unique canonical codes. Rules establish no legal authority.
    /// # Errors
    /// Rejects malformed, duplicate, unsorted or excessive input before copying.
    pub fn new(raw: &[&str], canonical: &[u16]) -> Result<Self> {
        if raw.len() > 16
            || canonical.len() > 16
            || raw.iter().any(|s| !code(s))
            || raw.windows(2).any(|w| matches!(w,[a,b] if a>=b))
            || canonical.windows(2).any(|w| matches!(w,[a,b] if a>=b))
        {
            return Err(IexAcceptanceError::Configuration);
        }
        Ok(Self {
            raw: raw.iter().map(|s| (*s).to_owned()).collect(),
            canonical: canonical.to_vec(),
        })
    }
}
fn code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 8 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// Source identity partition; provider IDs are never global sequences.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
#[non_exhaustive]
pub enum IexIdentityScope {
    /// One instrument/exchange/tape/session partition for source trade IDs.
    Trade {
        /// Exact owner-reviewed source exchange code.
        exchange: String,
        /// Exact owner-reviewed source tape code.
        tape: String,
    },
    /// One instrument/session partition for exact Unix minute bucket starts.
    MinuteBar,
}
/// Complete reviewed reference/window/identity input. The owner allocates namespaces
/// across sessions/profiles; this library can verify uniqueness only within this profile.
#[derive(Clone)]
pub struct IexIdentityLane {
    /// Exact selected listing/mapping version, including complete value and provenance.
    pub mapping: ReferenceVersion<InstrumentMapping>,
    /// Explicit exchange-local date/calendar; no host timezone or calendar extrapolation.
    pub calendar: ReferenceVersion<CalendarDay>,
    /// Explicit source-time window, at most 48 hours. May include reviewed extended hours;
    /// it never reclassifies raw intraday bars as core-session aggregates.
    pub window: EvidenceInterval,
    /// Owner-reserved high 64 bits of the logical observation identity; never reused for
    /// another source/instrument/session/exchange/tape/kind partition.
    pub namespace: NonZeroU64,
    /// Exact native partition matching the profile's single evidence kind.
    pub scope: IexIdentityScope,
}
/// Fresh authenticated reference histories supplied by the trusted composition root.
/// Merely constructing this borrow authenticates neither the catalogs nor their checksums.
#[derive(Clone, Copy)]
pub struct IexAcceptanceReferences<'a> {
    /// Complete current mapping corrections for the selected instruments.
    pub mappings: &'a ReferenceHistory<InstrumentMapping>,
    /// Complete current calendar corrections for the reviewed dates.
    pub calendars: &'a ReferenceHistory<CalendarDay>,
}
/// Frozen versioned owner interpretation. Bytes are a configuration commitment,
/// not independent approval. PR20C2 must verify the actual installed admission.
pub struct IexCanonicalProfile {
    source: SourceId,
    contract: EvidenceContract,
    transformation: TransformationRevision,
    lanes: Vec<IexIdentityLane>,
    rules: Vec<IexConditionRule>,
    bytes: Vec<u8>,
    digest: EvidenceDigest,
}
/// Canonical value plus untouched native normalized provenance. Neither is ReadyEvidence.
/// Retaining only the canonical value would discard bar VWAP/count or original condition order.
#[derive(Clone)]
pub struct IexAcceptedObservation {
    canonical: CanonicalObservation,
    native: IexObservation,
    profile: EvidenceDigest,
}
impl IexAcceptedObservation {
    /// Canonical revision with unchanged event/receipt times and no fabricated sequence.
    #[must_use]
    pub const fn canonical(&self) -> &CanonicalObservation {
        &self.canonical
    }
    /// Original normalized metadata and local capture coordinates, not raw wire bytes.
    #[must_use]
    pub const fn native(&self) -> &IexObservation {
        &self.native
    }
    /// Explicit owner-profile commitment; not a read or publication capability.
    #[must_use]
    pub const fn profile(&self) -> &EvidenceDigest {
        &self.profile
    }
    /// Move all provenance together into later bounded publication composition.
    #[must_use]
    pub fn into_parts(self) -> (CanonicalObservation, IexObservation, EvidenceDigest) {
        (self.canonical, self.native, self.profile)
    }
}
impl IexCanonicalProfile {
    /// Freeze 1..=128 unique identity partitions and at most 64 distinct condition rules.
    /// Input order does not change canonical bytes; duplicate partitions/namespaces reject.
    /// Only snapshot IEX trades OR raw minute bars with nanosecond source time are supported.
    /// # Errors
    /// Rejects unsupported semantics, closed/foreign dates, duplicate/ambiguous rules or
    /// profile encoding above 64 KiB. All windows/namespaces/rules are explicit owner input.
    pub fn new(
        source: SourceId,
        contract: EvidenceContract,
        transformation: TransformationRevision,
        lanes: &[IexIdentityLane],
        rules: &[IexConditionRule],
    ) -> Result<Self> {
        let kind = contract.shape().0;
        if contract.shape().1 != MarketScope::Venue(Venue::Xiex)
            || contract.rules().1 != TimestampPrecision::Nanoseconds
            || !matches!(
                kind,
                EvidenceKind::ShareTrade | EvidenceKind::RawBar(BarCoverage::Intraday)
            )
            || lanes.is_empty()
            || lanes.len() > 128
            || rules.len() > 64
            || (kind == EvidenceKind::ShareTrade && rules.is_empty())
            || (kind != EvidenceKind::ShareTrade && !rules.is_empty())
        {
            return Err(IexAcceptanceError::Configuration);
        }
        for (i, lane) in lanes.iter().enumerate() {
            let (_, venue, dates) = lane.mapping.value().binding();
            let (calendar_venue, date) = lane.calendar.value().key();
            let (start, end) = lane.window.bounds();
            let span = (i128::from(end.unix_parts().0) - i128::from(start.unix_parts().0))
                * 1_000_000_000
                + i128::from(end.unix_parts().1)
                - i128::from(start.unix_parts().1);
            let valid_scope = match &lane.scope {
                IexIdentityScope::Trade { exchange, tape } => {
                    kind == EvidenceKind::ShareTrade && code(exchange) && code(tape)
                }
                IexIdentityScope::MinuteBar => kind == EvidenceKind::RawBar(BarCoverage::Intraday),
            };
            if !valid_scope
                || venue != calendar_venue
                || !dates.contains(date)
                || span > 172_800_000_000_000
                || !matches!(
                    lane.calendar.value().hours(),
                    SessionHours::Regular(_) | SessionHours::EarlyClose(_)
                )
                || lanes.iter().take(i).any(|old| {
                    old.namespace == lane.namespace
                        || (old.mapping.value().identities().0
                            == lane.mapping.value().identities().0
                            && old.scope == lane.scope)
                })
            {
                return Err(IexAcceptanceError::Configuration);
            }
            if lanes.iter().take(i).any(|old| {
                old.mapping.value().identities().0 == lane.mapping.value().identities().0
                    && (!same(&old.mapping, &lane.mapping)
                        || !same(&old.calendar, &lane.calendar)
                        || old.window != lane.window)
            }) {
                return Err(IexAcceptanceError::Configuration);
            }
        }
        if rules
            .iter()
            .enumerate()
            .any(|(i, r)| rules.iter().take(i).any(|old| old.raw == r.raw))
        {
            return Err(IexAcceptanceError::Configuration);
        }
        let mut lanes = lanes.to_vec();
        lanes.sort_by(|a, b| {
            a.mapping
                .value()
                .identities()
                .0
                .cmp(&b.mapping.value().identities().0)
                .then_with(|| a.scope.cmp(&b.scope))
        });
        let mut rules = rules.to_vec();
        rules.sort_by(|a, b| a.raw.cmp(&b.raw));
        let bytes = encoding::encode(source, contract, transformation, &lanes, &rules)?;
        if bytes.len() > 65_536 {
            return Err(IexAcceptanceError::Configuration);
        }
        let digest = DailyEvidenceArtifact::hash(&bytes);
        Ok(Self {
            source,
            contract,
            transformation,
            lanes,
            rules,
            bytes,
            digest,
        })
    }
    /// Canonical private profile bytes, including full reference values. Never log them.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Pin this interpretation in the independently approved session configuration.
    #[must_use]
    pub const fn digest(&self) -> &EvidenceDigest {
        &self.digest
    }
}
fn same<T: PartialEq>(a: &ReferenceVersion<T>, b: &ReferenceVersion<T>) -> bool {
    a.id() == b.id()
        && a.revision() == b.revision()
        && a.source() == b.source()
        && a.known_at() == b.known_at()
        && a.checksum() == b.checksum()
        && a.value() == b.value()
}
macro_rules! redact {($($t:ty),*)=>{$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*};}
redact!(
    IexCanonicalProfile,
    IexConditionRule,
    IexIdentityLane,
    IexIdentityScope,
    IexAcceptanceReferences<'_>,
    IexAcceptedObservation
);
