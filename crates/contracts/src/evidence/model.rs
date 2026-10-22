use super::{EvidenceContractError as Error, redacted};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

macro_rules! text {
    ($name:ident,$doc:literal,$min:literal,$max:literal,$pattern:literal,$valid:expr) => {
        #[doc=$doc]
        #[derive(Clone,Eq,PartialEq,Ord,PartialOrd,Serialize)]
        #[serde(transparent)]
        #[cfg_attr(feature="schema",derive(schemars::JsonSchema))]
        pub struct $name(#[cfg_attr(feature="schema",schemars(length(min=$min,max=$max),regex(pattern=$pattern)))] pub(super) String);
        impl $name {
            /// Validate before allocating; never normalizes or truncates.
            /// # Errors
            /// Rejects malformed, oversized or noncanonical text.
            pub fn new(value:&str)->Result<Self,Error> {
                if value.len()<$min || value.len()>$max || !($valid)(value) { return Err(Error); }
                Ok(Self(value.to_owned()))
            }
            /// Explicit value access; diagnostic output stays redacted.
            #[must_use]
            pub fn as_str(&self)->&str { &self.0 }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error> {
                Self::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
        redacted!($name);
    }
}
text!(
    EvidenceAgeMillis,
    "Exact positive signed-millisecond reference-age ceiling, encoded as text.",
    1,
    19,
    "^[1-9][0-9]{0,18}$",
    |v: &str| !v.starts_with('0')
        && v.bytes().all(|b| b.is_ascii_digit())
        && v.parse::<i64>().is_ok()
);
fn hex(v: &str) -> bool {
    v.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
text!(
    EvidenceId,
    "Nonzero 128-bit lowercase hex transport identity; domain namespaces remain distinct.",
    32,
    32,
    "^[0-9a-f]*[1-9a-f][0-9a-f]*$",
    |v: &str| hex(v) && v.bytes().any(|b| b != b'0')
);
text!(
    EvidenceDigest,
    "Lowercase SHA-256; an all-zero digest is representable, unlike an identity.",
    64,
    64,
    "^[0-9a-f]{64}$",
    hex
);
text!(
    EvidenceRevision,
    "Exact nonzero u64 revision encoded as text, not a lossy JavaScript number.",
    1,
    20,
    "^[1-9][0-9]{0,19}$",
    |v: &str| !v.starts_with('0')
        && v.bytes().all(|b| b.is_ascii_digit())
        && v.parse::<u64>().is_ok()
);
text!(
    EvidenceDate,
    "Canonical Gregorian YYYY-MM-DD in years 1 through 9999; not proof of a session.",
    10,
    10,
    "^[0-9]{4}-[0-9]{2}-[0-9]{2}$",
    |v: &str| NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .is_ok_and(|d| d.year() > 0 && d.format("%Y-%m-%d").to_string() == v)
);
text!(
    EvidenceSequence,
    "Exact optional source sequence; zero is valid and scope comes from feed semantics.",
    1,
    20,
    "^(0|[1-9][0-9]{0,19})$",
    |v: &str| (v == "0" || !v.starts_with('0'))
        && v.bytes().all(|b| b.is_ascii_digit())
        && v.parse::<u64>().is_ok()
);
impl EvidenceDate {
    pub(super) fn date(&self) -> Result<NaiveDate, Error> {
        NaiveDate::parse_from_str(self.as_str(), "%Y-%m-%d").map_err(|_| Error)
    }
}

/// Lossless UTC seconds/nanoseconds, independent from event/receipt semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceTime {
    /// Signed Unix seconds within Gregorian years 1..9999.
    #[cfg_attr(feature="schema",schemars(range(min=-62135596800_i64,max=253402300799_i64)))]
    pub seconds: i64,
    /// Nanosecond fraction, never rounded to milliseconds.
    #[cfg_attr(feature = "schema", schemars(range(max = 999999999)))]
    pub nanos: u32,
}
impl EvidenceTime {
    pub(super) fn valid(self) -> bool {
        self.nanos < 1_000_000_000 && (-62_135_596_800..=253_402_300_799).contains(&self.seconds)
    }
    pub(super) fn millis(self) -> i128 {
        i128::from(self.seconds) * 1000 + i128::from(self.nanos / 1_000_000)
    }
}
/// Exact normalized nonnegative decimal; prices require positive coefficients.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceDecimal {
    /// 0 or 1..28 base-ten digits, with no leading zero/sign/exponent.
    #[cfg_attr(
        feature = "schema",
        schemars(length(min = 1, max = 28), regex(pattern = "^(0|[1-9][0-9]{0,27})$"))
    )]
    pub coefficient: String,
    /// Fractional scale; volume additionally requires <=6.
    #[cfg_attr(feature = "schema", schemars(range(max = 8)))]
    pub scale: u8,
}
impl EvidenceDecimal {
    pub(super) fn scaled(&self, max: u8, positive: bool) -> Result<i128, Error> {
        let s = &self.coefficient;
        if s.is_empty()
            || s.len() > 28
            || !s.bytes().all(|b| b.is_ascii_digit())
            || (s.len() > 1 && s.starts_with('0'))
            || self.scale > max
            || (self.scale > 0 && s.ends_with('0'))
            || (positive && s == "0")
        {
            return Err(Error);
        }
        s.parse::<i128>()
            .map(|n| n * 10_i128.pow(u32::from(8 - self.scale)))
            .map_err(|_| Error)
    }
}
/// Exact service-owned logical reference version, not a database location.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceVersion {
    /// Nonzero reference identity.
    pub id: EvidenceId,
    /// Exact selected revision.
    pub revision: EvidenceRevision,
}
/// Meaning of one selected reference witness; source bytes remain owner-retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceReferenceKind {
    /// Dated identity mapping.
    Mapping,
    /// Explicit open/closed calendar coverage.
    Calendar,
    /// Supported corporate-action annotation.
    Action,
    /// Complete action catalog coverage.
    ActionCoverage,
    /// Current acquisition rights review.
    Rights,
    /// Current provider capability review.
    ProviderReview,
}
/// Immutable provenance witness; checksum is of authenticated source bytes, not an ETag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceReference {
    /// Reference role.
    pub kind: EvidenceReferenceKind,
    /// Selected identity/revision.
    pub version: EvidenceVersion,
    /// Source authority identity.
    pub source: EvidenceId,
    /// Actual knowledge time.
    pub known_at: EvidenceTime,
    /// Source-byte digest; full referenced records must be retained for replay.
    pub source_sha256: EvidenceDigest,
    /// Required effective date for calendar/action roles, null for the others.
    #[serde(deserialize_with = "required")]
    pub date: Option<EvidenceDate>,
}
/// Authorization/storage scope frozen into object bytes, preventing cross-license deduplication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceScope {
    /// Immutable acquisition ID.
    pub request: EvidenceId,
    /// Digest of the immutable admitted request record, verified by its owner.
    pub request_sha256: EvidenceDigest,
    /// Authenticated principal.
    pub principal: EvidenceId,
    /// Data-only provider account.
    pub account: EvidenceId,
    /// Market source/feed identity.
    pub source: EvidenceId,
    /// Canonical instrument.
    pub instrument: EvidenceId,
    /// Canonical listing.
    pub listing: EvidenceId,
    /// Permitted audience.
    pub audience: EvidenceId,
    /// Permitted storage location/security scope.
    pub storage: EvidenceId,
    /// Exact reviewed endpoint catalog identity.
    pub endpoint: EvidenceId,
}
/// Exact versioned whole-series quality parameters, with no inferred defaults.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceQuality {
    /// Checked quality transformation revision.
    pub revision: EvidenceRevision,
    /// Positive maximum reference age.
    pub reference_age_ms: EvidenceAgeMillis,
    /// Inclusive consecutive-close basis-point threshold.
    pub close_change_bps: u16,
    /// Inclusive source-price basis-point threshold; no comparison source is included in v1.
    pub source_difference_bps: u16,
}
/// Explicit knowledge mode; retained means actual receipts preceded the cutoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceKnowledge {
    /// Current capture, not past availability.
    Capture,
    /// Previously retained observations.
    Retained,
}
/// Raw provider-day row, never adjusted or relabeled as core-only.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceBar {
    /// Exchange-local label.
    pub date: EvidenceDate,
    /// Source observation identity/revision.
    pub observation: EvidenceVersion,
    /// Linked source predecessor, null for original revision one.
    #[serde(deserialize_with = "required")]
    pub predecessor: Option<EvidenceRevision>,
    /// Exact row transformation revision.
    pub transformation: EvidenceRevision,
    /// Optional source sequence, preserving zero and the full u64 range as text.
    #[serde(deserialize_with = "required")]
    pub sequence: Option<EvidenceSequence>,
    /// Exact half-open provider bucket; not inferred from core hours.
    pub bucket: [EvidenceTime; 2],
    /// Open/high/low/close in USD/share, without rounding.
    pub ohlc: [EvidenceDecimal; 4],
    /// Reported nonnegative shares, zero permitted.
    pub volume: EvidenceDecimal,
    /// Source event, actual receipt, optional provider observation/publication times.
    pub event_at: EvidenceTime,
    /// Actual local receipt.
    pub received_at: EvidenceTime,
    /// Optional provider observation time; presence is required even when null.
    #[serde(deserialize_with = "required")]
    pub observed_at: Option<EvidenceTime>,
    /// Optional provider publication time; not independent historical proof.
    #[serde(deserialize_with = "required")]
    pub published_at: Option<EvidenceTime>,
    /// Reviewed, sorted source condition codes.
    #[cfg_attr(feature = "schema", schemars(length(max = 16)))]
    pub conditions: Vec<u16>,
    /// Selected mapping version.
    pub mapping: EvidenceVersion,
    /// Selected calendar version.
    pub calendar: EvidenceVersion,
    /// Exact supported action annotations, never applied to prices.
    #[cfg_attr(feature = "schema", schemars(length(max = 4096)))]
    pub actions: Vec<EvidenceVersion>,
}
/// Fixed canonical artifact schema version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceFormat {
    /// Strict daily-v1 canonical JSON, lexicographically sorted keys.
    #[serde(rename = "edgeagent.evidence.daily.v1")]
    V1,
}
/// Untrusted construction/wire input. Only `DailyEvidenceArtifact` validates and seals it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DailyEvidence {
    /// Exact immutable format (raw USD provider-day equity data only).
    pub version: EvidenceFormat,
    /// Scope and immutable request binding.
    pub scope: EvidenceScope,
    /// Exact trade-eligibility/feed semantic revision.
    pub semantics: EvidenceVersion,
    /// Half-open requested exchange-local dates, including explicitly closed days.
    pub range: [EvidenceDate; 2],
    /// Explicit knowledge interpretation.
    pub knowledge: EvidenceKnowledge,
    /// Inclusive evidence knowledge cutoff.
    pub cutoff: EvidenceTime,
    /// Actual capture/freeze time, not source label time.
    pub captured_at: EvidenceTime,
    /// Exclusive licensed retention deadline, never longer than 24 hours after capture.
    pub delete_by: EvidenceTime,
    /// Exact checked quality configuration.
    pub quality: EvidenceQuality,
    /// Complete selected reference census, canonically sorted by role/version.
    #[cfg_attr(feature = "schema", schemars(length(min = 5, max = 4717)))]
    pub references: Vec<EvidenceReference>,
    /// Strictly date-ordered accepted rows; calendar witnesses cover closed days too.
    #[cfg_attr(feature = "schema", schemars(length(min = 1, max = 252)))]
    pub bars: Vec<EvidenceBar>,
    /// Predecessor content digest for an append-only correction, null for original capture.
    #[serde(deserialize_with = "required")]
    pub replaces: Option<EvidenceDigest>,
}
pub(super) fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
redacted!(EvidenceDecimal, EvidenceBar);
