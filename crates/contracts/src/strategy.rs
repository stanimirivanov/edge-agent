//! Immutable configuration declarations; decoding grants no approval or execution authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum canonical bundle size, including its final newline.
pub const MAX_BUNDLE_BYTES: usize = 16_384;
/// Payload-safe configuration rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationError;
impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid strategy configuration")
    }
}
impl std::error::Error for ConfigurationError {}

/// Feature census in canonical declaration order; these names assert requirements, not facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// Complete raw USD closes.
    Closes,
    /// Qualified daily opens, highs, lows and closes.
    Ohlc,
    /// Qualified daily share volume.
    Volume,
    /// Explicit owner-authorized event coverage.
    EventCoverage,
}

/// Strict strategy definition. Revision identifiers name separately retained specifications.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StrategyDefinition {
    /// Stable strategy identifier, independent of implementation digest.
    pub id: String,
    /// Immutable implementation digest.
    pub implementation: String,
    /// Exact formula revision; availability must be checked by the loader.
    pub score_revision: String,
    /// Exact eligibility specification revision.
    pub eligibility_revision: String,
    /// Sorted unique complete required-feature census.
    pub required_features: Vec<Feature>,
    /// Lookback intervals, requiring lookback+1 sessions.
    pub lookback_sessions: u16,
    /// Holding horizon in trading sessions, independent of lookback and TTL.
    pub horizon_sessions: u16,
    /// Artifact lifetime in milliseconds from decision time, bounded by evidence rights at runtime.
    pub expiry_ms: u32,
    /// Explicit benchmark instrument identity; does not assert benchmark evidence exists.
    pub benchmark: String,
    /// Exact deterministic regime specification revision.
    pub regime_revision: String,
    /// Exact invalidation specification revision.
    pub invalidation_revision: String,
    /// Compatibility with the current concrete ranking contract; no arbitrary scoring units.
    pub ranking: Ranking,
}
/// Qualified score units, basis and tie-break semantics.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Ranking {
    /// Raw USD return ppm descending, instrument ascending on rounded-score ties.
    RawUsdPpmDescendingInstrumentAscending,
}
/// Research compatibility explicitly selected by the configuration.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    /// Existing research request and artifact v1 contracts.
    ResearchV1,
}
/// Supported research direction; new directions require contract review.
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Long-only research, without order or account authority.
    LongOnly,
}
/// Independent policy declarations, never chosen by a strategy result.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolicyDefinition {
    /// Immutable specification digest, separate from strategy implementation identity.
    pub revision: String,
    /// Sorted unique explicitly allowed instrument tokens.
    pub instruments: Vec<String>,
    /// Explicit allowed direction.
    pub direction: Direction,
    /// Inclusive holding-horizon ceiling in trading sessions.
    pub max_horizon_sessions: u16,
    /// Inclusive evidence-age budget in milliseconds.
    pub max_age_ms: u32,
    /// Exact liquidity formula/rounding revision.
    pub liquidity_revision: String,
    /// Minimum mean daily dollar turnover in whole USD cents.
    pub min_daily_turnover_cents: u64,
    /// Exact volatility formula/window/rounding revision.
    pub volatility_revision: String,
    /// Inclusive maximum volatility, dimensionless ppm under the selected formula.
    pub max_volatility_ppm: u32,
    /// Exact gap formula/rounding revision.
    pub gap_revision: String,
    /// Inclusive maximum absolute gap, dimensionless ppm.
    pub max_gap_ppm: u32,
    /// Event exclusion window before decision time in milliseconds.
    pub event_before_ms: u32,
    /// Event exclusion window after decision time in milliseconds.
    pub event_after_ms: u32,
    /// Exact evidence-quality specification revision.
    pub quality_revision: String,
}
/// Closed versioned bundle. No defaults, aliases or unknown-field preservation.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum Configuration {
    /// First immutable strategy/policy configuration contract.
    #[serde(rename = "research.configuration.v1")]
    V1 {
        /// Transport compatibility, not approval state.
        compatibility: Compatibility,
        /// Strategy requirements and deterministic behavior specifications.
        strategy: StrategyDefinition,
        /// Independent constraints.
        policy: PolicyDefinition,
    },
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes().any(|b| b != b'0')
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Configuration {
    /// Validate declarations and encode recursively sorted JSON with one final LF.
    /// Formula names are opaque specifications, not proof of installed implementations.
    /// # Errors
    /// Rejects invalid bounds, duplicate/unsorted selections or incompatible declarations.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ConfigurationError> {
        let Self::V1 {
            strategy: s,
            policy: p,
            ..
        } = self;
        if !token(&s.id) || !digest(&s.implementation) || !digest(&p.revision)
            || [&s.score_revision, &s.eligibility_revision, &s.benchmark, &s.regime_revision,
                &s.invalidation_revision, &p.liquidity_revision, &p.volatility_revision,
                &p.gap_revision, &p.quality_revision].iter().any(|v| !token(v))
            || !(1..=252).contains(&s.lookback_sessions)
            || !(1..=252).contains(&s.horizon_sessions)
            || !(1..=252).contains(&p.max_horizon_sessions)
            || s.horizon_sessions > p.max_horizon_sessions
            || !(1..=300_000).contains(&s.expiry_ms)
            || !(1..=2_678_400_000).contains(&p.max_age_ms)
            || p.instruments.is_empty() || p.instruments.len() > 64
            || p.instruments.iter().any(|v| !token(v))
            || !p.instruments.windows(2).all(|w| w[0] < w[1])
            // Every v1 policy includes liquidity, gaps, volatility and events.
            || s.required_features != [Feature::Closes, Feature::Ohlc, Feature::Volume, Feature::EventCoverage]
        {
            return Err(ConfigurationError);
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(self).map_err(|_| ConfigurationError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(ConfigurationError);
        }
        Ok(bytes)
    }
    /// Validate exact canonical input; alternate encodings and duplicate keys reject.
    /// # Errors
    /// Rejects oversize input before parsing and all unsupported fields/versions/values.
    pub fn decode(bytes: &[u8]) -> Result<Self, ConfigurationError> {
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(ConfigurationError);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| ConfigurationError)?;
        if value.canonical_bytes()? != bytes {
            return Err(ConfigurationError);
        }
        Ok(value)
    }
    /// Identity of exact validated bytes, excluding mutable approval/lifecycle state.
    /// # Errors
    /// Rejects invalid declarations. A digest grants no authority.
    pub fn digest(&self) -> Result<String, ConfigurationError> {
        Ok(Sha256::digest(self.canonical_bytes()?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}
