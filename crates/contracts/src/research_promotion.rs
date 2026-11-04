//! Owner-reviewed evidence bound to one Research approval.
use crate::{
    research::{digest, token},
    strategy_governance::Profile,
};
use serde::{Deserialize, Serialize};

pub const MAX_PROMOTION_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPromotion;
impl std::fmt::Display for InvalidPromotion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Research promotion evidence")
    }
}
impl std::error::Error for InvalidPromotion {}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub profile: Profile,
    pub family: String,
    pub trial: String,
    /// SHA-256 of the selected artifact's canonical Research record.
    pub result: String,
    /// SHA-256 of the externally retained, owner-reviewed holdout report.
    pub report: String,
    pub holdout_start_ms: i64,
    pub holdout_end_ms: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum Wire<T> {
    #[serde(rename = "research.promotion.v1")]
    V1(T),
}

impl Promotion {
    pub fn bytes(&self) -> Result<Vec<u8>, InvalidPromotion> {
        self.profile.bytes().map_err(|_| InvalidPromotion)?;
        if !token(&self.family)
            || !token(&self.trial)
            || [&self.result, &self.report]
                .into_iter()
                .any(|d| !digest(d) || d.bytes().all(|b| b == b'0'))
            || self.holdout_start_ms < 0
            || self.holdout_end_ms <= self.holdout_start_ms
        {
            return Err(InvalidPromotion);
        }
        let mut bytes = crate::messaging::canonical_json::to_vec(&Wire::V1(self))
            .map_err(|_| InvalidPromotion)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_PROMOTION_BYTES {
            return Err(InvalidPromotion);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, InvalidPromotion> {
        if bytes.len() > MAX_PROMOTION_BYTES {
            return Err(InvalidPromotion);
        }
        let Wire::V1(value): Wire<Self> =
            serde_json::from_slice(bytes).map_err(|_| InvalidPromotion)?;
        if value.bytes()? != bytes {
            return Err(InvalidPromotion);
        }
        Ok(value)
    }
}
