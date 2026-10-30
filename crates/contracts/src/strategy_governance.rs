//! Exact legacy-profile approval binding; not the full strategy/policy bundle.
use serde::{Deserialize, Serialize};
/// Immutable M04 profile, retained before explicit owner approval.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub scope: String,
    pub implementation: String,
    pub configuration: String,
    pub lookback_sessions: u16,
    pub max_age_ms: i64,
    pub ttl_ms: i64,
    pub formula: String,
    pub renderer: String,
}
impl Profile {
    /// Canonical legacy profile v1 bytes (type version carried by the owning table).
    pub fn bytes(&self) -> Result<Vec<u8>, crate::strategy::ConfigurationError> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        };
        let digest = |s: &str| {
            s.len() == 64
                && s.bytes().any(|b| b != b'0')
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.formula != "raw-usd-roc-ppm-half-even-v1"
            || self.renderer != "research-ranking-raw-v1"
            || !token(&self.scope)
            || !token(&self.formula)
            || !token(&self.renderer)
            || !digest(&self.implementation)
            || !digest(&self.configuration)
            || !(1..=252).contains(&self.lookback_sessions)
            || !(1..=2_678_400_000).contains(&self.max_age_ms)
            || !(1..=300_000).contains(&self.ttl_ms)
        {
            return Err(crate::strategy::ConfigurationError);
        }
        let mut bytes = crate::messaging::canonical_json::to_vec(self)
            .map_err(|_| crate::strategy::ConfigurationError)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
    /// Decode only bounded exact canonical bytes, rejecting duplicate/unknown fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, crate::strategy::ConfigurationError> {
        if bytes.len() > 4096 {
            return Err(crate::strategy::ConfigurationError);
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| crate::strategy::ConfigurationError)?;
        if value.bytes()? != bytes {
            return Err(crate::strategy::ConfigurationError);
        }
        Ok(value)
    }
}
