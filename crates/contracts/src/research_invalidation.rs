//! Bounded owner checks reference retained requests, never caller-supplied market facts.
use crate::research::{ResearchRecord, ResearchRequest};
use serde::{Deserialize, Serialize};
/// Bound before parsing, including the final canonical newline.
pub const MAX_COMMAND_BYTES: usize = 16_384;
/// Safe command rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCommand;
impl std::fmt::Display for InvalidCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research invalidation command")
    }
}
impl std::error::Error for InvalidCommand {}
/// Explicit effective time; this command neither schedules work nor acquires evidence.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "research.invalidation-check.v1")]
    V1 {
        original: ResearchRequest,
        effective_at_ms: i64,
        reassessment: Option<ResearchRequest>,
    },
}
impl Command {
    pub fn bytes(&self) -> Result<Vec<u8>, InvalidCommand> {
        let Self::V1 {
            original,
            effective_at_ms,
            reassessment,
        } = self;
        ResearchRecord::Request(original.clone())
            .canonical_bytes()
            .map_err(|_| InvalidCommand)?;
        if *effective_at_ms < original.decision_at_ms {
            return Err(InvalidCommand);
        }
        if let Some(next) = reassessment {
            ResearchRecord::Request(next.clone())
                .canonical_bytes()
                .map_err(|_| InvalidCommand)?;
            if next.scope != original.scope
                || next.implementation != original.implementation
                || next.configuration != original.configuration
                || next.instruments != original.instruments
                || next.lookback_sessions != original.lookback_sessions
                || next.request_id == original.request_id
                || next.decision_at_ms <= original.decision_at_ms
                || next.decision_at_ms > *effective_at_ms
            {
                return Err(InvalidCommand);
            }
        }
        let mut bytes =
            crate::messaging::canonical_json::to_vec(self).map_err(|_| InvalidCommand)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(InvalidCommand);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, InvalidCommand> {
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(InvalidCommand);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| InvalidCommand)?;
        if value.bytes()? != bytes {
            return Err(InvalidCommand);
        }
        Ok(value)
    }
}
