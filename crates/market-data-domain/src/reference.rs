use std::{collections::BTreeMap, fmt};

use crate::{ReferenceId, ReferenceRevision, SourceId, UtcInstant};

/// Payload-free reference construction and resolution failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReferenceError {
    /// An effective interval or UTC session window is invalid.
    InvalidInterval,
    /// A symbol or named timezone-data revision violates its bounded syntax.
    InvalidToken,
    /// More than 4096 reference versions were supplied to one history.
    Capacity,
    /// A record repeats a revision or its revision/receipt ordering contradicts history.
    RevisionConflict,
    /// No known reference covers the requested key and effective date.
    Missing,
    /// Distinct current records claim the same requested mapping or calendar day.
    Ambiguous,
    /// Known corporate-action metadata requires unsupported identity/adjustment handling.
    UnsupportedAction,
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInterval => "invalid reference interval",
            Self::InvalidToken => "invalid reference token",
            Self::Capacity => "reference history capacity exceeded",
            Self::RevisionConflict => "conflicting reference revisions",
            Self::Missing => "reference coverage unavailable at cutoff",
            Self::Ambiguous => "ambiguous reference coverage",
            Self::UnsupportedAction => "unsupported corporate action",
        })
    }
}
impl std::error::Error for ReferenceError {}

/// Immutable observation of one logical reference record. Corrections reuse `id`.
/// `known_at` is actual local receipt, not an effective date or a vendor claim.
/// The caller verifies source authority and SHA-256 of the source bytes; this
/// value records that provenance, not proof of acquisition or entitlement.
#[derive(Clone, Eq, PartialEq)]
pub struct ReferenceVersion<T> {
    id: ReferenceId,
    revision: ReferenceRevision,
    source: SourceId,
    known_at: UtcInstant,
    checksum: [u8; 32],
    value: T,
}

impl<T> ReferenceVersion<T> {
    /// Record a received observation with its source checksum and checked identity.
    #[must_use]
    pub const fn new(
        id: ReferenceId,
        revision: ReferenceRevision,
        source: SourceId,
        known_at: UtcInstant,
        checksum: [u8; 32],
        value: T,
    ) -> Self {
        Self {
            id,
            revision,
            source,
            known_at,
            checksum,
            value,
        }
    }
    /// Logical identity shared by all corrections of this record.
    #[must_use]
    pub const fn id(&self) -> ReferenceId {
        self.id
    }
    /// Service-assigned revision; gaps are allowed.
    #[must_use]
    pub const fn revision(&self) -> ReferenceRevision {
        self.revision
    }
    /// Source catalog identity, not provider-specific metadata.
    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    /// Actual local receipt establishing the earliest usable knowledge cutoff.
    #[must_use]
    pub const fn known_at(&self) -> UtcInstant {
        self.known_at
    }
    /// Caller-verified SHA-256 of source bytes; explicit access, never diagnostic output.
    #[must_use]
    pub const fn checksum(&self) -> &[u8; 32] {
        &self.checksum
    }
    /// Borrow the selected immutable reference value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

impl<T> fmt::Debug for ReferenceVersion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReferenceVersion")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("source", &self.source)
            .field("known_at", &self.known_at)
            .finish_non_exhaustive()
    }
}

/// Bounded immutable revision history, not a persistence repository or complete
/// security master. Callers retain every revision needed by historical reads.
///
/// ```
/// use edgeagent_market_data_domain::{
///     ReferenceHistory, ReferenceId, ReferenceRevision, ReferenceVersion, SourceId, UtcInstant,
/// };
/// let id = ReferenceId::new(1)?;
/// let source = SourceId::new(1)?;
/// let first_receipt = UtcInstant::from_unix_millis(10)?;
/// let correction_receipt = UtcInstant::from_unix_millis(20)?;
/// let history = ReferenceHistory::new(vec![
///     ReferenceVersion::new(id, ReferenceRevision::new(1)?, source, first_receipt, [1; 32], "original"),
///     ReferenceVersion::new(id, ReferenceRevision::new(2)?, source, correction_receipt, [2; 32], "corrected"),
/// ])?;
/// assert_eq!(history.at(first_receipt).map(|v| *v.value()).collect::<Vec<_>>(), ["original"]);
/// assert_eq!(history.at(correction_receipt).map(|v| *v.value()).collect::<Vec<_>>(), ["corrected"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceHistory<T>(BTreeMap<ReferenceId, Vec<ReferenceVersion<T>>>);

impl<T> ReferenceHistory<T> {
    /// Validate at most 4096 versions, independent of input order. Duplicate
    /// revisions fail even when identical; normalize transport duplicates first.
    ///
    /// # Errors
    /// Returns `Capacity` for oversized input or `RevisionConflict` for repeated
    /// revisions or decreasing receipt time as revisions increase.
    pub fn new(mut versions: Vec<ReferenceVersion<T>>) -> Result<Self, ReferenceError> {
        if versions.len() > 4096 {
            return Err(ReferenceError::Capacity);
        }
        versions.sort_by_key(|v| (v.id, v.revision));
        let mut histories: BTreeMap<_, Vec<ReferenceVersion<T>>> = BTreeMap::new();
        for version in versions {
            let history = histories.entry(version.id).or_default();
            if history.last().is_some_and(|previous| {
                previous.revision == version.revision || previous.known_at > version.known_at
            }) {
                return Err(ReferenceError::RevisionConflict);
            }
            history.push(version);
        }
        Ok(Self(histories))
    }

    /// Select each record's latest received revision at the inclusive cutoff,
    /// in stable record-ID order. Effective-date filtering MUST follow selection;
    /// otherwise corrections moving an interval can resurrect obsolete facts.
    /// An empty result does not establish completeness of the source catalog.
    pub fn at(&self, cutoff: UtcInstant) -> impl Iterator<Item = &ReferenceVersion<T>> {
        self.0.values().filter_map(move |history| {
            history
                .iter()
                .rev()
                .find(|version| version.known_at <= cutoff)
        })
    }
}

pub(crate) fn exactly_one<T>(mut values: impl Iterator<Item = T>) -> Result<T, ReferenceError> {
    let first = values.next().ok_or(ReferenceError::Missing)?;
    if values.next().is_some() {
        return Err(ReferenceError::Ambiguous);
    }
    Ok(first)
}

pub(crate) fn token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}
