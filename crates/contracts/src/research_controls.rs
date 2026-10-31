//! Explicit operator commands; no client-supplied actor or timestamp is accepted.
use serde::{Deserialize, Serialize};
/// Maximum canonical control command including its final newline.
pub const MAX_CONTROL_BYTES: usize = 2048;
/// Safe shape refusal without payload diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlError;
impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research control command")
    }
}
impl std::error::Error for ControlError {}
/// Targets compose: either disabled switch blocks fresh completion.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Scope,
    Implementation { digest: String },
}
/// Revision one may explicitly enable or disable; no history means enabled.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "research.control.v1")]
    V1 {
        scope: String,
        target: Target,
        revision: u64,
        enabled: bool,
        reason_code: String,
    },
}
impl Command {
    pub fn bytes(&self) -> Result<Vec<u8>, ControlError> {
        let Self::V1 {
            scope,
            target,
            revision,
            reason_code,
            ..
        } = self;
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        };
        if !token(scope)
            || !token(reason_code)
            || *revision == 0
            || *revision > i64::MAX as u64
            || matches!(target,Target::Implementation{digest} if digest.len()!=64 || !digest.bytes().any(|b|b!=b'0') || !digest.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            return Err(ControlError);
        }
        let mut bytes = crate::messaging::canonical_json::to_vec(self).map_err(|_| ControlError)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_CONTROL_BYTES {
            return Err(ControlError);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ControlError> {
        if bytes.len() > MAX_CONTROL_BYTES {
            return Err(ControlError);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| ControlError)?;
        if value.bytes()? != bytes {
            return Err(ControlError);
        }
        Ok(value)
    }
}
