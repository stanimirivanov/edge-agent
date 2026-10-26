use super::*;
use crate::{Component, EventRetention, MessageDefinition};
use serde::{Deserialize, Serialize};

/// Opaque capture notification, not evidence readiness or a read capability.
pub const STREAM_PUBLISHED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.market-data.stream-segment-published.v1",
    "urn:edgeagent:schema:market-data:stream-segment-published:v1",
    Component::MarketData,
    "stream",
    // Opaque audit identity outlives the finite session plus licensed capture lifetime.
    EventRetention::Audit,
);

/// No licensed fingerprint, source, instrument, location or observation leaves the owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StreamPublished {
    /// Opaque owner-assigned session intent.
    pub session: EvidenceId,
    /// Opaque owner-assigned publication intent, stable across recovery.
    pub publication: EvidenceId,
}
