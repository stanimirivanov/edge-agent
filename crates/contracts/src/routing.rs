//! Versioned routing, ownership, partition, retention, and size contracts.

use crate::message_type::{is_lowercase_token, parse_message_type};
use crate::messaging::MAX_METADATA_LENGTH;
use crate::{Component, MessageEnvelope, MessageMetadata};
use serde::Serialize;
use url::Url;

mod error;
mod registry;

pub use self::error::MessageRoutingError;
pub use self::registry::MessageRegistry;

const COMMAND_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
const WORKFLOW_EVENT_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;
const AUDIT_EVENT_RETENTION_SECONDS: u64 = 365 * 24 * 60 * 60;

/// Maximum encoded envelope accepted by every portable broker profile.
pub const MAX_PORTABLE_MESSAGE_BYTES: usize = 256 * 1024;

/// Semantic role of a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKind {
    /// Intent with exactly one owning handler.
    Command,
    /// Completed fact with exactly one authoritative producer.
    Event,
}

impl MessageKind {
    const fn subject_token(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Event => "event",
        }
    }

    /// Return the portable namespace shared by every subject of this kind.
    #[must_use]
    pub const fn subject_namespace(self) -> &'static str {
        match self {
            Self::Command => "edgeagent.command",
            Self::Event => "edgeagent.event",
        }
    }
}

/// Portable delivery behavior required from a broker profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    /// Deliver each command to one member of the owning handler group.
    WorkQueue,
    /// Retain facts for independent durable consumers and replay.
    RetainedStream,
}

/// Meaning of the configured retention duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetentionMode {
    /// Retain until acknowledged or the maximum age expires.
    UntilAcknowledgedOrExpired,
    /// Retain for at least the declared replay window.
    ReplayWindow,
}

/// Portable retention classes used by all transport profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetentionClass {
    /// Commands remain recoverable for at most seven days before expiry.
    Command,
    /// Workflow facts remain replayable for at least thirty days.
    WorkflowEvent,
    /// Audit facts remain replayable for at least 365 days.
    AuditEvent,
}

/// Retention choices valid only for completed events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventRetention {
    /// Keep an ordinary workflow fact replayable for at least thirty days.
    Workflow,
    /// Keep a material audit fact replayable for at least 365 days.
    Audit,
}

impl EventRetention {
    const fn class(self) -> RetentionClass {
        match self {
            Self::Workflow => RetentionClass::WorkflowEvent,
            Self::Audit => RetentionClass::AuditEvent,
        }
    }
}

impl RetentionClass {
    /// Return the mode that gives the duration its meaning.
    #[must_use]
    pub const fn mode(self) -> RetentionMode {
        match self {
            Self::Command => RetentionMode::UntilAcknowledgedOrExpired,
            Self::WorkflowEvent | Self::AuditEvent => RetentionMode::ReplayWindow,
        }
    }

    /// Return the portable retention duration in seconds.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        match self {
            Self::Command => COMMAND_RETENTION_SECONDS,
            Self::WorkflowEvent => WORKFLOW_EVENT_RETENTION_SECONDS,
            Self::AuditEvent => AUDIT_EVENT_RETENTION_SECONDS,
        }
    }
}

/// Static routing and compatibility definition for one message major version.
/// The fields cannot be changed after construction; call [`Self::validate`] or
/// put definitions in a [`MessageRegistry`] to validate textual invariants.
///
/// ```compile_fail
/// use edgeagent_contracts::{Component, MessageDefinition};
/// let mut definition = MessageDefinition::command(
///     "com.edgeagent.execution.submit-dry-run-order.v1",
///     "urn:edgeagent:schema:submit-dry-run-order:v1",
///     Component::ExecutionSimulator,
///     "order",
/// );
/// definition.message_type = "com.edgeagent.execution.other.v1";
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageDefinition {
    /// CloudEvents type including the payload major version.
    pub message_type: &'static str,
    /// Immutable absolute URI for the payload schema.
    pub data_schema: &'static str,
    /// Command or event semantics.
    pub kind: MessageKind,
    /// Sole command handler or authoritative event producer.
    pub owner: Component,
    /// Aggregate namespace required at the start of `partitionkey`.
    pub partition_prefix: &'static str,
    /// Portable retention behavior.
    pub retention: RetentionClass,
}

impl MessageDefinition {
    /// Define a command owned by exactly one handler component.
    #[must_use]
    pub const fn command(
        message_type: &'static str,
        data_schema: &'static str,
        owner: Component,
        partition_prefix: &'static str,
    ) -> Self {
        Self {
            message_type,
            data_schema,
            kind: MessageKind::Command,
            owner,
            partition_prefix,
            retention: RetentionClass::Command,
        }
    }

    /// Define an event produced authoritatively by one component.
    ///
    /// The event-only retention type prevents command retention from being
    /// assigned to an event.
    ///
    /// ```compile_fail
    /// use edgeagent_contracts::{Component, MessageDefinition, RetentionClass};
    /// let _ = MessageDefinition::event(
    ///     "com.edgeagent.execution.dry-run-order-accepted.v1",
    ///     "urn:edgeagent:schema:dry-run-order-accepted:v1",
    ///     Component::ExecutionSimulator,
    ///     "order",
    ///     RetentionClass::Command,
    /// );
    /// ```
    #[must_use]
    pub const fn event(
        message_type: &'static str,
        data_schema: &'static str,
        owner: Component,
        partition_prefix: &'static str,
        retention: EventRetention,
    ) -> Self {
        Self {
            message_type,
            data_schema,
            kind: MessageKind::Event,
            owner,
            partition_prefix,
            retention: retention.class(),
        }
    }

    /// Return the exact CloudEvents type, including the payload major version.
    #[must_use]
    pub const fn message_type(self) -> &'static str {
        self.message_type
    }

    /// Return the immutable absolute payload-schema URI.
    #[must_use]
    pub const fn data_schema(self) -> &'static str {
        self.data_schema
    }

    /// Return whether this definition describes a command or an event.
    #[must_use]
    pub const fn kind(self) -> MessageKind {
        self.kind
    }

    /// Return the sole handler or authoritative producer.
    #[must_use]
    pub const fn owner(self) -> Component {
        self.owner
    }

    /// Return the required aggregate namespace for `partitionkey`.
    #[must_use]
    pub const fn partition_prefix(self) -> &'static str {
        self.partition_prefix
    }

    /// Return the portable retention class.
    #[must_use]
    pub const fn retention(self) -> RetentionClass {
        self.retention
    }

    /// Return portable queue or stream delivery behavior.
    #[must_use]
    pub const fn delivery_mode(self) -> DeliveryMode {
        match self.kind {
            MessageKind::Command => DeliveryMode::WorkQueue,
            MessageKind::Event => DeliveryMode::RetainedStream,
        }
    }

    /// Derive the stable NATS-compatible subject for this definition.
    ///
    /// # Errors
    ///
    /// Returns an error when the definition's type name is invalid.
    pub fn subject(self) -> Result<String, MessageRoutingError> {
        let parts = definition_type(self.message_type)?;
        Ok(format!(
            "edgeagent.{}.{}.{}.{}",
            self.kind.subject_token(),
            parts.domain,
            parts.name,
            parts.version
        ))
    }

    /// Validate the static definition itself.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, schemas, partitions, or a retention
    /// class that conflicts with command/event semantics.
    pub fn validate(self) -> Result<(), MessageRoutingError> {
        definition_type(self.message_type)?;
        if self.data_schema.len() > MAX_METADATA_LENGTH {
            return Err(invalid_definition("data_schema", "exceeds 512 bytes"));
        }
        if !self.data_schema.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(invalid_definition(
                "data_schema",
                "must contain visible ASCII without spaces",
            ));
        }
        Url::parse(self.data_schema)
            .map_err(|_| invalid_definition("data_schema", "must be an absolute URI"))?;
        if !is_lowercase_token(self.partition_prefix) {
            return Err(invalid_definition(
                "partition_prefix",
                "must be a lowercase ASCII token",
            ));
        }
        match (self.kind, self.retention) {
            (MessageKind::Command, RetentionClass::Command)
            | (MessageKind::Event, RetentionClass::WorkflowEvent | RetentionClass::AuditEvent) => {
                Ok(())
            }
            _ => Err(invalid_definition(
                "retention",
                "does not match command/event semantics",
            )),
        }
    }

    /// Build an envelope and enforce this routing definition.
    ///
    /// # Errors
    ///
    /// Returns an envelope or routing error from construction and validation.
    pub fn build<T: Serialize>(
        self,
        metadata: MessageMetadata,
        payload: &T,
    ) -> Result<MessageEnvelope, MessageRoutingError> {
        self.validate()?;
        let envelope = MessageEnvelope::from_payload(metadata, payload)?;
        self.validate_envelope_validated(&envelope)?;
        Ok(envelope)
    }

    /// Validate a decoded envelope against type, schema, ownership, partition,
    /// and portable size policy. The definition is also validated here; use a
    /// [`MessageRegistry`] to validate static definitions once at startup.
    ///
    /// # Errors
    ///
    /// Returns a mismatch or size error when the envelope is not eligible for
    /// this route.
    pub fn validate_envelope(self, envelope: &MessageEnvelope) -> Result<(), MessageRoutingError> {
        self.validate()?;
        self.validate_envelope_validated(envelope)
    }

    // Only call after the immutable definition passed validation at this
    // boundary or when its containing registry was constructed.
    fn validate_envelope_validated(
        self,
        envelope: &MessageEnvelope,
    ) -> Result<(), MessageRoutingError> {
        require_equal("type", self.message_type, envelope.message_type())?;
        require_equal(
            "dataschema",
            self.data_schema,
            envelope.data_schema().unwrap_or("<missing>"),
        )?;
        let partition_key = envelope.partition_key().unwrap_or("<missing>");
        let partition_suffix = partition_key
            .strip_prefix(self.partition_prefix)
            .and_then(|value| value.strip_prefix('/'));
        if partition_suffix.is_none_or(str::is_empty) {
            return Err(MessageRoutingError::ContractMismatch {
                field: "partitionkey",
            });
        }
        if self.kind == MessageKind::Event {
            require_equal("source", self.owner.source_uri(), envelope.source())?;
        }
        let encoded_bytes = envelope.to_json()?.len();
        if encoded_bytes > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(MessageRoutingError::EnvelopeTooLarge {
                actual_bytes: encoded_bytes,
                maximum_bytes: MAX_PORTABLE_MESSAGE_BYTES,
            });
        }
        Ok(())
    }
}

fn require_equal(
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), MessageRoutingError> {
    if expected == actual {
        Ok(())
    } else {
        Err(MessageRoutingError::ContractMismatch { field })
    }
}

const fn invalid_definition(field: &'static str, reason: &'static str) -> MessageRoutingError {
    MessageRoutingError::InvalidDefinition { field, reason }
}

fn definition_type(
    value: &str,
) -> Result<crate::message_type::MessageTypeParts<'_>, MessageRoutingError> {
    if value.len() > MAX_METADATA_LENGTH {
        return Err(invalid_definition("message_type", "exceeds 512 bytes"));
    }
    parse_message_type(value)
        .ok_or_else(|| invalid_definition("message_type", "is not a valid EdgeAgent type"))
}

#[cfg(test)]
mod tests;
