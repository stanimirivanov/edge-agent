//! MarketData-owned research evidence handoff. References never grant object access.
use crate::{
    Component, EventRetention, MessageDefinition, MessageEnvelope, MessageRegistry,
    MessageRoutingError,
    evidence::{EvidenceDecision, EvidenceDecisionData, EvidenceOutcome},
    research::{AbstentionReason, ResearchContractError as Error, ResearchRecord, ResearchRequest},
};
/// Ingress requests MarketData evidence, not direct Research evaluation.
pub const REQUEST: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.market-data.fulfil-research-evidence.v1",
    "urn:edgeagent:schema:research:record:v1",
    Component::MarketData,
    "research-request",
);
/// Correlated reference-only terminal evidence result; no prices or fetch locations.
pub const DECIDED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.market-data.research-evidence-decided.v1",
    "urn:edgeagent:schema:market-data:research-evidence:v1",
    Component::MarketData,
    "research-request",
    EventRetention::Workflow,
);
const DEFINITIONS: [MessageDefinition; 4] = [
    REQUEST,
    DECIDED,
    crate::evidence::DEFINITIONS[0],
    crate::evidence::DEFINITIONS[1],
];
/// Exact offline handoff plus existing acquisition routes; provisioning is explicit.
/// # Errors
/// Rejects invalid compiled definitions.
pub fn registry() -> Result<MessageRegistry<'static>, MessageRoutingError> {
    MessageRegistry::new(&DEFINITIONS)
}
/// Untrusted wire DTO; call validate/from_envelope before using references.
#[derive(Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ResearchEvidence {
    /// An existing owner terminal receipt identifies retained evidence, not read permission.
    Ready {
        /// Original immutable request.
        request: ResearchRequest,
        /// Exact owner acquisition decision.
        decision: Box<EvidenceDecisionData>,
    },
    /// No usable evidence; no object or provider diagnostic can accompany this result.
    Abstained {
        /// Original immutable request.
        request: ResearchRequest,
        /// Stable research refusal vocabulary.
        reason: AbstentionReason,
    },
}
/// Stable correlation key and partition.
/// # Errors
/// Rejects malformed research contracts.
pub fn binding(request: &ResearchRequest) -> Result<(String, String), Error> {
    let record = ResearchRecord::Request(request.clone());
    record.canonical_bytes()?;
    Ok((
        record.digest()?,
        format!("research-request/{}/{}", request.scope, request.request_id),
    ))
}
fn bound(envelope: &MessageEnvelope, request: &ResearchRequest) -> Result<(), Error> {
    let (key, partition) = binding(request)?;
    if envelope.partition_key() != Some(partition.as_str())
        || envelope.subject() != Some(partition.as_str())
        || envelope.extension("idempotencykey") != Some(key.as_str())
        || envelope.extension("correlationid") != Some(key.as_str())
    {
        return Err(Error);
    }
    Ok(())
}
/// Decode a Gateway-produced request; the outer adapter authenticates its producer.
/// # Errors
/// Rejects wrong producer/route, foreign correlation and non-request records.
pub fn request(envelope: &MessageEnvelope) -> Result<ResearchRequest, Error> {
    REQUEST.validate_envelope(envelope).map_err(|_| Error)?;
    if envelope.source() != Component::Gateway.source_uri() {
        return Err(Error);
    }
    let ResearchRecord::Request(request) = envelope.payload().map_err(|_| Error)? else {
        return Err(Error);
    };
    bound(envelope, &request)?;
    Ok(request)
}
impl ResearchEvidence {
    /// Original caller-scoped request.
    #[must_use]
    pub const fn request(&self) -> &ResearchRequest {
        match self {
            Self::Ready { request, .. } | Self::Abstained { request, .. } => request,
        }
    }
    /// Structural/cross-field validation, not proof of current authority or persistence.
    /// # Errors
    /// Rejects invalid requests, invalid object metadata or a rejected decision labelled ready.
    pub fn validate(&self) -> Result<(), Error> {
        binding(self.request())?;
        if let Self::Ready { decision, .. } = self {
            let validated =
                EvidenceDecision::try_from(decision.as_ref().clone()).map_err(|_| Error)?;
            if !matches!(validated.data().outcome, EvidenceOutcome::Ready { .. }) {
                return Err(Error);
            }
        }
        Ok(())
    }
    /// Validate owner source, exact route and caller/request correlation before consumption.
    /// # Errors
    /// Rejects malformed or incorrectly correlated owner outcomes.
    pub fn from_envelope(envelope: &MessageEnvelope) -> Result<Self, Error> {
        DECIDED.validate_envelope(envelope).map_err(|_| Error)?;
        let value: Self = envelope.payload().map_err(|_| Error)?;
        value.validate()?;
        bound(envelope, value.request())?;
        Ok(value)
    }
}
/// Local draft 2020-12 schema; cross-field checks remain mandatory.
/// # Errors
/// Rejects serialization failure.
#[cfg(feature = "schema")]
pub fn schema() -> Result<String, Error> {
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ResearchEvidence>();
    let mut value = serde_json::to_value(schema).map_err(|_| Error)?;
    value["$id"] = "urn:edgeagent:schema:market-data:research-evidence:v1".into();
    let bytes = crate::messaging::canonical_json::to_vec(&value).map_err(|_| Error)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| Error)?;
    Ok(serde_json::to_string_pretty(&value).map_err(|_| Error)? + "\n")
}
