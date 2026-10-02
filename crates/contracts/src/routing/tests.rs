use super::{
    DeliveryMode, EventRetention, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageRegistry,
    MessageRoutingError, RetentionClass, RetentionMode,
};
use crate::messaging::MAX_METADATA_LENGTH;
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
    EventRetention::Audit,
);

fn metadata(definition: MessageDefinition) -> MessageMetadata {
    MessageMetadata {
        id: "message-01".to_owned(),
        source: Component::ExecutionSimulator.source_uri().to_owned(),
        message_type: definition.message_type().to_owned(),
        subject: "order/order-01".to_owned(),
        time: "2026-09-26T00:00:00Z".to_owned(),
        data_schema: definition.data_schema().to_owned(),
        correlation_id: "correlation-01".to_owned(),
        causation_id: "command-01".to_owned(),
        idempotency_key: "order-01".to_owned(),
        partition_key: "order/order-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

#[test]
fn static_definition_respects_type_and_schema_limits() {
    let type_prefix = "com.edgeagent.research.";
    let type_suffix = ".v1";
    let name = "a".repeat(MAX_METADATA_LENGTH - type_prefix.len() - type_suffix.len());
    let at_limit: &'static str =
        Box::leak(format!("{type_prefix}{name}{type_suffix}").into_boxed_str());
    let over_limit: &'static str =
        Box::leak(format!("{type_prefix}{name}a{type_suffix}").into_boxed_str());
    let schema_prefix = "urn:edgeagent:schema:";
    let schema_name = "a".repeat(MAX_METADATA_LENGTH - schema_prefix.len());
    let schema_at_limit: &'static str =
        Box::leak(format!("{schema_prefix}{schema_name}").into_boxed_str());
    let schema_over_limit: &'static str =
        Box::leak(format!("{schema_prefix}{schema_name}a").into_boxed_str());

    let mut definition = COMMAND;
    definition.message_type = at_limit;
    assert!(definition.validate().is_ok());
    definition.message_type = over_limit;
    assert!(matches!(
        definition.validate(),
        Err(MessageRoutingError::InvalidDefinition {
            field: "message_type",
            ..
        })
    ));
    assert!(definition.subject().is_err());

    definition.message_type = COMMAND.message_type;
    definition.data_schema = schema_at_limit;
    assert!(definition.validate().is_ok());
    definition.data_schema = schema_over_limit;
    assert!(matches!(
        definition.validate(),
        Err(MessageRoutingError::InvalidDefinition {
            field: "data_schema",
            ..
        })
    ));
}

#[test]
fn command_definition_derives_work_queue_subject() -> Result<(), Box<dyn Error>> {
    COMMAND.validate()?;

    assert_eq!(
        COMMAND.subject()?,
        "edgeagent.command.execution.submit-dry-run-order.v1"
    );
    assert_eq!(COMMAND.delivery_mode(), DeliveryMode::WorkQueue);
    assert_eq!(COMMAND.kind().subject_namespace(), "edgeagent.command");
    assert_eq!(
        COMMAND.retention().mode(),
        RetentionMode::UntilAcknowledgedOrExpired
    );
    assert_eq!(COMMAND.retention().seconds(), 604_800);
    assert_eq!(COMMAND.owner(), Component::ExecutionSimulator);
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
    assert_eq!(EVENT.kind().subject_namespace(), "edgeagent.event");
    assert_eq!(EVENT.retention().mode(), RetentionMode::ReplayWindow);
    assert_eq!(EVENT.retention().seconds(), 31_536_000);
    let workflow = MessageDefinition::event(
        EVENT.message_type(),
        EVENT.data_schema(),
        EVENT.owner(),
        EVENT.partition_prefix(),
        EventRetention::Workflow,
    );
    assert_eq!(workflow.retention(), RetentionClass::WorkflowEvent);
    assert_eq!(workflow.retention().seconds(), 2_592_000);
    Ok(())
}

#[test]
fn registry_rejects_duplicate_types() {
    let result = MessageRegistry::new(&[COMMAND, COMMAND]);

    assert!(matches!(
        result,
        Err(MessageRoutingError::DuplicateMessageType)
    ));
}

#[test]
fn registry_rejects_invalid_definition_at_construction() {
    let invalid = MessageDefinition {
        data_schema: "relative/schema",
        ..COMMAND
    };
    assert!(matches!(
        MessageRegistry::new(&[invalid]),
        Err(MessageRoutingError::InvalidDefinition {
            field: "data_schema",
            ..
        })
    ));
}

#[test]
fn registry_and_standalone_checks_agree_for_validated_definitions() -> Result<(), Box<dyn Error>> {
    let definitions = [COMMAND, EVENT];
    let registry = MessageRegistry::new(&definitions)?;

    let mut wrong_schema = metadata(COMMAND);
    wrong_schema.data_schema = "urn:edgeagent:schema:other:v1".to_owned();
    let mut wrong_partition = metadata(COMMAND);
    wrong_partition.partition_key = "account/account-01".to_owned();
    let mut wrong_producer = metadata(EVENT);
    wrong_producer.source = Component::Gateway.source_uri().to_owned();

    for (definition, metadata) in [
        (COMMAND, metadata(COMMAND)),
        (COMMAND, wrong_schema),
        (COMMAND, wrong_partition),
        (EVENT, metadata(EVENT)),
        (EVENT, wrong_producer),
    ] {
        let envelope = crate::MessageEnvelope::from_payload(metadata, &json!({}))?;
        assert_eq!(
            registry.validate(&envelope).map(|_| ()),
            definition.validate_envelope(&envelope)
        );
    }
    Ok(())
}

#[test]
fn event_rejects_command_retention() {
    // Private fields let the contract test model a corrupt internal definition;
    // external callers cannot construct this combination.
    let invalid = MessageDefinition {
        retention: RetentionClass::Command,
        ..EVENT
    };

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
        Err(MessageRoutingError::UnsupportedMessageType)
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
fn routing_errors_do_not_echo_untrusted_metadata() -> Result<(), Box<dyn Error>> {
    const SENTINEL: &str = "private-schema-sentinel-7391";
    let mut wrong_schema = metadata(COMMAND);
    wrong_schema.data_schema = format!("urn:edgeagent:schema:{SENTINEL}");
    let envelope = crate::MessageEnvelope::from_payload(wrong_schema, &json!({}))?;
    let Err(error) = COMMAND.validate_envelope(&envelope) else {
        return Err("wrong schema must fail".into());
    };

    assert!(!error.to_string().contains(SENTINEL));
    assert!(!format!("{error:?}").contains(SENTINEL));

    let mut unsupported = metadata(COMMAND);
    unsupported.message_type = format!("com.edgeagent.{SENTINEL}.request.v1");
    let envelope = crate::MessageEnvelope::from_payload(unsupported, &json!({}))?;
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let Err(error) = registry.validate(&envelope) else {
        return Err("unregistered type must fail".into());
    };

    assert!(matches!(error, MessageRoutingError::UnsupportedMessageType));
    assert!(!error.to_string().contains(SENTINEL));
    assert!(!format!("{error:?}").contains(SENTINEL));
    Ok(())
}

#[test]
fn nested_contract_error_chain_remains_payload_safe() -> Result<(), Box<dyn Error>> {
    const SENTINEL: &str = "private-parser-sentinel-7391";
    let input = format!("{{\"type\": {SENTINEL}}}");
    let Err(contract) = crate::MessageEnvelope::from_json(input.as_bytes()) else {
        return Err("malformed input must fail".into());
    };
    let routing = MessageRoutingError::from(contract);

    assert!(matches!(routing, MessageRoutingError::Envelope(_)));
    assert!(!routing.to_string().contains(SENTINEL));
    assert!(!format!("{routing:?}").contains(SENTINEL));
    assert!(
        routing
            .source()
            .is_some_and(|source| !source.to_string().contains(SENTINEL))
    );
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
    let envelope =
        crate::MessageEnvelope::from_payload(metadata(COMMAND), &json!({"content": oversized}))?;

    let result = COMMAND.validate_envelope(&envelope);

    assert!(matches!(
        result,
        Err(MessageRoutingError::EnvelopeTooLarge { .. })
    ));
    Ok(())
}
