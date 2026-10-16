use std::{fmt, sync::LazyLock};

use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;

use crate::{AcceptanceError, schemas::SchemaKind};

const DEFINITIONS: [MessageDefinition; 2] = [
    MessageDefinition::command(
        "com.edgeagent.conformance.apply-probe.v1",
        "urn:edgeagent:schema:conformance:apply-probe:v1",
        Component::Research,
        "probe",
    ),
    MessageDefinition::event(
        "com.edgeagent.conformance.probe-applied.v1",
        "urn:edgeagent:schema:conformance:probe-applied:v1",
        Component::Research,
        "probe",
        EventRetention::Workflow,
    ),
];

static REGISTRY: LazyLock<Result<MessageRegistry<'static>, AcceptanceError>> =
    LazyLock::new(|| MessageRegistry::new(&DEFINITIONS).map_err(|_| AcceptanceError::Definition));

/// Return the two conformance-only definitions in command/fact order.
///
/// Existing component identities label synthetic producer/handler roles only;
/// this does not register a production research capability.
#[must_use]
pub const fn definitions() -> &'static [MessageDefinition; 2] {
    &DEFINITIONS
}

/// Return the validated, process-local conformance registry.
///
/// # Errors
/// Returns [`AcceptanceError::Definition`] for an invalid compiled-in definition.
pub fn registry() -> Result<MessageRegistry<'static>, AcceptanceError> {
    *REGISTRY
}

// Keep the schema and wire model together. Unknown optional fields are ignored
// by old readers, but the original MessageEnvelope is never rewritten on decode.
#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename = "ApplyProbe")]
pub(crate) struct CommandPayload {
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[a-z0-9][a-z0-9_-]*$"))]
    pub(crate) probe_id: String,
    #[schemars(range(min = 0, max = 1_000_000))]
    pub(crate) expected_revision: u32,
    #[schemars(range(min = 0, max = 1_000_000))]
    pub(crate) value: u32,
}

#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename = "ProbeApplied")]
pub(crate) struct FactPayload {
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[a-z0-9][a-z0-9_-]*$"))]
    pub(crate) probe_id: String,
    #[schemars(range(min = 1, max = 1_000_001))]
    pub(crate) revision: u32,
    #[schemars(range(min = 0, max = 1_000_000))]
    pub(crate) value: u32,
}

/// Validated synthetic intent to set one probe's value at an expected revision.
///
/// This is a conformance payload, not a research request or trading command.
///
/// ```
/// use edgeagent_event_spine_conformance::ApplyProbe;
/// let command = ApplyProbe::new("probe-01", 0, 42)?;
/// assert_eq!(command.expected_revision(), 0);
/// # Ok::<(), edgeagent_event_spine_conformance::AcceptanceError>(())
/// ```
///
/// ```compile_fail,E0616
/// use edgeagent_event_spine_conformance::ApplyProbe;
/// let command = ApplyProbe::new("probe-01", 0, 42)?;
/// let _payload = command.0;
/// # Ok::<(), edgeagent_event_spine_conformance::AcceptanceError>(())
/// ```
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ApplyProbe(CommandPayload);

impl ApplyProbe {
    /// Construct a bounded synthetic command (revision/value 0..=1,000,000).
    ///
    /// # Errors
    /// Returns [`AcceptanceError::Payload`] for invalid identity or numeric bounds.
    pub fn new(
        probe_id: impl Into<String>,
        expected_revision: u32,
        value: u32,
    ) -> Result<Self, AcceptanceError> {
        let payload = CommandPayload {
            probe_id: probe_id.into(),
            expected_revision,
            value,
        };
        SchemaKind::Command.validate(&to_value(&payload)?)?;
        Ok(Self(payload))
    }

    /// Return the bounded aggregate identifier, without the partition prefix.
    #[must_use]
    pub fn probe_id(&self) -> &str {
        &self.0.probe_id
    }

    /// Return the required predecessor revision; zero means initial state.
    #[must_use]
    pub const fn expected_revision(&self) -> u32 {
        self.0.expected_revision
    }

    /// Return the requested synthetic integer value.
    #[must_use]
    pub const fn value(&self) -> u32 {
        self.0.value
    }

    /// Wrap the payload in caller-owned metadata and verify its complete route.
    ///
    /// # Errors
    /// Rejects invalid metadata, wrong message kind/schema, and partition mismatch.
    pub fn envelope(&self, metadata: MessageMetadata) -> Result<MessageEnvelope, AcceptanceError> {
        encode(metadata, &self.0, SchemaKind::Command, self.probe_id())
    }
}

/// Validated synthetic fact recording one committed probe revision (1..=1,000,001).
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProbeApplied(FactPayload);

impl ProbeApplied {
    /// Construct a bounded synthetic fact; value is 0..=1,000,000.
    ///
    /// # Errors
    /// Returns [`AcceptanceError::Payload`] for invalid identity or numeric bounds.
    pub fn new(
        probe_id: impl Into<String>,
        revision: u32,
        value: u32,
    ) -> Result<Self, AcceptanceError> {
        let payload = FactPayload {
            probe_id: probe_id.into(),
            revision,
            value,
        };
        SchemaKind::Fact.validate(&to_value(&payload)?)?;
        Ok(Self(payload))
    }

    /// Return the bounded aggregate identifier.
    #[must_use]
    pub fn probe_id(&self) -> &str {
        &self.0.probe_id
    }

    /// Return the resulting committed revision.
    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.0.revision
    }

    /// Return the resulting synthetic integer value.
    #[must_use]
    pub const fn value(&self) -> u32 {
        self.0.value
    }

    /// Wrap the fact in caller-owned metadata and verify its producer/route.
    ///
    /// # Errors
    /// Rejects invalid metadata, wrong message kind/schema, and partition mismatch.
    pub fn envelope(&self, metadata: MessageMetadata) -> Result<MessageEnvelope, AcceptanceError> {
        encode(metadata, &self.0, SchemaKind::Fact, self.probe_id())
    }
}

impl fmt::Debug for ApplyProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApplyProbe").finish_non_exhaustive()
    }
}
impl fmt::Debug for ProbeApplied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProbeApplied").finish_non_exhaustive()
    }
}

impl<'de> Deserialize<'de> for ApplyProbe {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)
            .map_err(|_| D::Error::custom(AcceptanceError::Payload))?;
        decode_command(value).map_err(D::Error::custom)
    }
}
impl<'de> Deserialize<'de> for ProbeApplied {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)
            .map_err(|_| D::Error::custom(AcceptanceError::Payload))?;
        decode_fact(value).map_err(D::Error::custom)
    }
}

/// The closed current acceptance workflow, after envelope and payload validation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcceptanceMessage {
    /// Synthetic intent, not yet a committed transition.
    Command(ApplyProbe),
    /// Synthetic fact describing a committed transition.
    Fact(ProbeApplied),
}

impl AcceptanceMessage {
    /// Decode bounded untrusted structured bytes without resolving schema URIs.
    ///
    /// # Errors
    /// Returns bounded envelope, routing, payload, or partition errors. No
    /// typed message is returned until every check passes; no state is mutated.
    pub fn from_json(bytes: &[u8]) -> Result<Self, AcceptanceError> {
        let envelope = MessageEnvelope::from_json(bytes).map_err(|_| AcceptanceError::Envelope)?;
        Self::from_envelope(&envelope)
    }

    /// Validate a borrowed envelope without changing its bytes or extensions.
    ///
    /// # Errors
    /// Returns routing/payload/partition errors before yielding a typed message.
    pub fn from_envelope(envelope: &MessageEnvelope) -> Result<Self, AcceptanceError> {
        registry()?
            .validate(envelope)
            .map_err(|_| AcceptanceError::Routing)?;
        let value = envelope
            .payload::<Value>()
            .map_err(|_| AcceptanceError::Payload)?;
        if envelope.message_type() == DEFINITIONS[0].message_type() {
            let command = decode_command(value)?;
            validate_partition(envelope, command.probe_id())?;
            Ok(Self::Command(command))
        } else if envelope.message_type() == DEFINITIONS[1].message_type() {
            let fact = decode_fact(value)?;
            validate_partition(envelope, fact.probe_id())?;
            Ok(Self::Fact(fact))
        } else {
            // New definitions must gain an explicit typed decoder, never fall
            // through to the existing fact payload merely because they route.
            Err(AcceptanceError::Routing)
        }
    }
}

fn decode_command(value: Value) -> Result<ApplyProbe, AcceptanceError> {
    SchemaKind::Command.validate(&value)?;
    serde_json::from_value(value)
        .map(ApplyProbe)
        .map_err(|_| AcceptanceError::Payload)
}

fn decode_fact(value: Value) -> Result<ProbeApplied, AcceptanceError> {
    SchemaKind::Fact.validate(&value)?;
    serde_json::from_value(value)
        .map(ProbeApplied)
        .map_err(|_| AcceptanceError::Payload)
}

fn to_value(payload: &impl Serialize) -> Result<Value, AcceptanceError> {
    serde_json::to_value(payload).map_err(|_| AcceptanceError::Payload)
}

fn encode(
    metadata: MessageMetadata,
    payload: &impl Serialize,
    kind: SchemaKind,
    probe_id: &str,
) -> Result<MessageEnvelope, AcceptanceError> {
    let envelope =
        MessageEnvelope::from_payload(metadata, payload).map_err(|_| AcceptanceError::Envelope)?;
    if envelope.message_type() != kind.definition().message_type() {
        return Err(AcceptanceError::Routing);
    }
    registry()?
        .validate(&envelope)
        .map_err(|_| AcceptanceError::Routing)?;
    validate_partition(&envelope, probe_id)?;
    Ok(envelope)
}

fn validate_partition(envelope: &MessageEnvelope, probe_id: &str) -> Result<(), AcceptanceError> {
    let expected = format!("probe/{probe_id}");
    if envelope.partition_key() == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(AcceptanceError::Partition)
    }
}
