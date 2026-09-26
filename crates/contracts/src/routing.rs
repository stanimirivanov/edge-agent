//! Versioned routing, ownership, partition, retention, and size contracts.

use crate::message_type::{is_lowercase_token, parse_message_type};
use crate::{Component, MessageContractError, MessageEnvelope, MessageMetadata};
use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};
use url::Url;

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
    #[must_use]
    pub const fn event(
        message_type: &'static str,
        data_schema: &'static str,
        owner: Component,
        partition_prefix: &'static str,
        retention: RetentionClass,
    ) -> Self {
        Self {
            message_type,
            data_schema,
            kind: MessageKind::Event,
            owner,
            partition_prefix,
            retention,
        }
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
        let parts = parse_message_type(self.message_type)
            .ok_or_else(|| invalid_definition("message_type", "is not a valid EdgeAgent type"))?;
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
        parse_message_type(self.message_type)
            .ok_or_else(|| invalid_definition("message_type", "is not a valid EdgeAgent type"))?;
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
        self.validate_envelope(&envelope)?;
        Ok(envelope)
    }

    /// Validate a decoded envelope against type, schema, ownership, partition,
    /// and portable size policy.
    ///
    /// # Errors
    ///
    /// Returns a mismatch or size error when the envelope is not eligible for
    /// this route.
    pub fn validate_envelope(self, envelope: &MessageEnvelope) -> Result<(), MessageRoutingError> {
        self.validate()?;
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
                expected: format!("{}/<identifier>", self.partition_prefix),
                actual: partition_key.to_owned(),
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

/// Validated lookup table for the message definitions supported by a process.
#[derive(Clone, Copy, Debug)]
pub struct MessageRegistry<'definitions> {
    definitions: &'definitions [MessageDefinition],
}

impl<'definitions> MessageRegistry<'definitions> {
    /// Validate definitions and reject duplicate message types or subjects.
    ///
    /// # Errors
    ///
    /// Returns an error when a definition is invalid or two entries collide.
    pub fn new(
        definitions: &'definitions [MessageDefinition],
    ) -> Result<Self, MessageRoutingError> {
        for (index, definition) in definitions.iter().enumerate() {
            definition.validate()?;
            let subject = definition.subject()?;
            for previous in &definitions[..index] {
                if previous.message_type == definition.message_type {
                    return Err(MessageRoutingError::DuplicateMessageType(
                        definition.message_type,
                    ));
                }
                if previous.subject()? == subject {
                    return Err(MessageRoutingError::DuplicateSubject(subject));
                }
            }
        }
        Ok(Self { definitions })
    }

    /// Resolve one exact major-version message type.
    #[must_use]
    pub fn resolve(&self, message_type: &str) -> Option<&'definitions MessageDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.message_type == message_type)
    }

    /// Return every validated definition in declaration order.
    #[must_use]
    pub const fn definitions(&self) -> &'definitions [MessageDefinition] {
        self.definitions
    }

    /// Resolve and validate an untrusted envelope.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported message type or contract mismatch.
    pub fn validate(
        &self,
        envelope: &MessageEnvelope,
    ) -> Result<&'definitions MessageDefinition, MessageRoutingError> {
        let definition = self.resolve(envelope.message_type()).ok_or_else(|| {
            MessageRoutingError::UnsupportedMessageType(envelope.message_type().to_owned())
        })?;
        definition.validate_envelope(envelope)?;
        Ok(definition)
    }
}

/// Routing definition or envelope eligibility failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageRoutingError {
    /// A static routing definition is internally invalid.
    InvalidDefinition {
        /// Invalid definition field.
        field: &'static str,
        /// Stable explanation.
        reason: &'static str,
    },
    /// Two definitions declare the same message type.
    DuplicateMessageType(&'static str),
    /// Two definitions resolve to the same transport subject.
    DuplicateSubject(String),
    /// No definition accepts the envelope's exact major-version type.
    UnsupportedMessageType(String),
    /// Envelope metadata conflicts with its registered definition.
    ContractMismatch {
        /// Mismatching envelope field.
        field: &'static str,
        /// Definition-owned value.
        expected: String,
        /// Envelope-provided value.
        actual: String,
    },
    /// Encoded structured message exceeds the portable limit.
    EnvelopeTooLarge {
        /// Actual encoded length.
        actual_bytes: usize,
        /// Maximum portable length.
        maximum_bytes: usize,
    },
    /// Envelope construction or encoding failed.
    Envelope(MessageContractError),
}

impl Display for MessageRoutingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDefinition { field, reason } => {
                write!(formatter, "invalid message definition {field}: {reason}")
            }
            Self::DuplicateMessageType(message_type) => {
                write!(formatter, "duplicate message type: {message_type}")
            }
            Self::DuplicateSubject(subject) => write!(formatter, "duplicate subject: {subject}"),
            Self::UnsupportedMessageType(message_type) => {
                write!(formatter, "unsupported message type: {message_type}")
            }
            Self::ContractMismatch {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "message {field} mismatch: expected {expected}, got {actual}"
            ),
            Self::EnvelopeTooLarge {
                actual_bytes,
                maximum_bytes,
            } => write!(
                formatter,
                "message envelope is {actual_bytes} bytes; portable maximum is {maximum_bytes}"
            ),
            Self::Envelope(error) => write!(formatter, "message envelope error: {error}"),
        }
    }
}

impl Error for MessageRoutingError {}

impl From<MessageContractError> for MessageRoutingError {
    fn from(error: MessageContractError) -> Self {
        Self::Envelope(error)
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
        Err(MessageRoutingError::ContractMismatch {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

const fn invalid_definition(field: &'static str, reason: &'static str) -> MessageRoutingError {
    MessageRoutingError::InvalidDefinition { field, reason }
}

#[cfg(test)]
mod tests {
    use super::{
        DeliveryMode, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageRegistry,
        MessageRoutingError, RetentionClass, RetentionMode,
    };
    use crate::{Component, MessageMetadata};
    use serde_json::json;
    use std::error::Error;

    const COMMAND: MessageDefinition = MessageDefinition::command(
        "com.edgeagent.execution.submit-dry-run-order.v1",
        "urn:edgeagent:schema:submit-dry-run-order:v1",
        Component::ExecutionSimulator,
        "order",
    );
    const EVENT: MessageDefinition = MessageDefinition::event(
        "com.edgeagent.execution.dry-run-order-accepted.v1",
        "urn:edgeagent:schema:dry-run-order-accepted:v1",
        Component::ExecutionSimulator,
        "order",
        RetentionClass::AuditEvent,
    );

    fn metadata(definition: MessageDefinition) -> MessageMetadata {
        MessageMetadata {
            id: "message-01".to_owned(),
            source: Component::ExecutionSimulator.source_uri().to_owned(),
            message_type: definition.message_type.to_owned(),
            subject: "order/order-01".to_owned(),
            time: "2026-09-26T00:00:00Z".to_owned(),
            data_schema: definition.data_schema.to_owned(),
            correlation_id: "correlation-01".to_owned(),
            causation_id: "command-01".to_owned(),
            idempotency_key: "order-01".to_owned(),
            partition_key: "order/order-01".to_owned(),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        }
    }

    #[test]
    fn command_definition_derives_work_queue_subject() -> Result<(), Box<dyn Error>> {
        COMMAND.validate()?;

        assert_eq!(
            COMMAND.subject()?,
            "edgeagent.command.execution.submit-dry-run-order.v1"
        );
        assert_eq!(COMMAND.delivery_mode(), DeliveryMode::WorkQueue);
        assert_eq!(COMMAND.kind.subject_namespace(), "edgeagent.command");
        assert_eq!(
            COMMAND.retention.mode(),
            RetentionMode::UntilAcknowledgedOrExpired
        );
        assert_eq!(COMMAND.retention.seconds(), 604_800);
        assert_eq!(COMMAND.owner, Component::ExecutionSimulator);
        Ok(())
    }

    #[test]
    fn event_definition_derives_retained_subject() -> Result<(), Box<dyn Error>> {
        EVENT.validate()?;

        assert_eq!(
            EVENT.subject()?,
            "edgeagent.event.execution.dry-run-order-accepted.v1"
        );
        assert_eq!(EVENT.delivery_mode(), DeliveryMode::RetainedStream);
        assert_eq!(EVENT.kind.subject_namespace(), "edgeagent.event");
        assert_eq!(EVENT.retention.mode(), RetentionMode::ReplayWindow);
        assert_eq!(EVENT.retention.seconds(), 31_536_000);
        Ok(())
    }

    #[test]
    fn registry_rejects_duplicate_types() {
        let result = MessageRegistry::new(&[COMMAND, COMMAND]);

        assert!(matches!(
            result,
            Err(MessageRoutingError::DuplicateMessageType(
                "com.edgeagent.execution.submit-dry-run-order.v1"
            ))
        ));
    }

    #[test]
    fn event_rejects_command_retention() {
        let invalid = MessageDefinition::event(
            EVENT.message_type,
            EVENT.data_schema,
            EVENT.owner,
            EVENT.partition_prefix,
            RetentionClass::Command,
        );

        assert!(matches!(
            invalid.validate(),
            Err(MessageRoutingError::InvalidDefinition {
                field: "retention",
                ..
            })
        ));
    }

    #[test]
    fn registry_rejects_unknown_major_version() -> Result<(), Box<dyn Error>> {
        let definitions = [COMMAND];
        let registry = MessageRegistry::new(&definitions)?;
        let mut unsupported = metadata(COMMAND);
        unsupported.message_type = "com.edgeagent.execution.submit-dry-run-order.v2".to_owned();
        let envelope = crate::MessageEnvelope::from_payload(unsupported, &json!({}))?;

        let result = registry.validate(&envelope);

        assert!(matches!(
            result,
            Err(MessageRoutingError::UnsupportedMessageType(_))
        ));
        Ok(())
    }

    #[test]
    fn definition_rejects_wrong_schema_and_partition() -> Result<(), Box<dyn Error>> {
        let mut wrong_schema = metadata(COMMAND);
        wrong_schema.data_schema = "urn:edgeagent:schema:other:v1".to_owned();
        let envelope = crate::MessageEnvelope::from_payload(wrong_schema, &json!({}))?;
        assert!(matches!(
            COMMAND.validate_envelope(&envelope),
            Err(MessageRoutingError::ContractMismatch {
                field: "dataschema",
                ..
            })
        ));

        let mut wrong_partition = metadata(COMMAND);
        wrong_partition.partition_key = "account/account-01".to_owned();
        let envelope = crate::MessageEnvelope::from_payload(wrong_partition, &json!({}))?;
        assert!(matches!(
            COMMAND.validate_envelope(&envelope),
            Err(MessageRoutingError::ContractMismatch {
                field: "partitionkey",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn event_rejects_non_authoritative_source() -> Result<(), Box<dyn Error>> {
        let mut wrong_source = metadata(EVENT);
        wrong_source.source = Component::Gateway.source_uri().to_owned();
        let envelope = crate::MessageEnvelope::from_payload(wrong_source, &json!({}))?;

        let result = EVENT.validate_envelope(&envelope);

        assert!(matches!(
            result,
            Err(MessageRoutingError::ContractMismatch {
                field: "source",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn oversized_envelope_is_rejected() -> Result<(), Box<dyn Error>> {
        let oversized = "x".repeat(MAX_PORTABLE_MESSAGE_BYTES);
        let envelope = crate::MessageEnvelope::from_payload(
            metadata(COMMAND),
            &json!({"content": oversized}),
        )?;

        let result = COMMAND.validate_envelope(&envelope);

        assert!(matches!(
            result,
            Err(MessageRoutingError::EnvelopeTooLarge { .. })
        ));
        Ok(())
    }
}
