//! Public stored-envelope validation contracts, independent of a database or broker.

use edgeagent_contracts::{
    Component, EventRetention, MAX_PORTABLE_MESSAGE_BYTES, MessageContractError, MessageDefinition,
    MessageEnvelope, MessageMetadata, MessageRegistry, MessageRoutingError,
};
use edgeagent_messaging::{ClaimedMessage, LeaseGeneration};
use std::error::Error;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.research.request-received.v1",
    "urn:edgeagent:schema:request-received:v1",
    Component::Research,
    "request",
);
const EVENT: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.request-observed.v1",
    "urn:edgeagent:schema:request-observed:v1",
    Component::Research,
    "request",
    EventRetention::Workflow,
);
const PAYLOAD_SENTINEL: &str = "private-stored-payload-sentinel-7391";
const METADATA_SENTINEL: &str = "private-stored-metadata-sentinel-7391";
const ATTEMPT: u32 = 7;
const GENERATION: u64 = 11;

fn metadata(definition: MessageDefinition) -> MessageMetadata {
    MessageMetadata {
        id: "stored-envelope-message-01".to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: definition.message_type().to_owned(),
        subject: "request/stored-envelope-01".to_owned(),
        time: "2026-10-10T00:00:00Z".to_owned(),
        data_schema: definition.data_schema().to_owned(),
        correlation_id: "stored-envelope-correlation-01".to_owned(),
        causation_id: "stored-envelope-causation-01".to_owned(),
        idempotency_key: "stored-envelope-request-01".to_owned(),
        partition_key: "request/stored-envelope-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

fn stored_claim(
    envelope: &MessageEnvelope,
    transport_subject: String,
    bytes: Vec<u8>,
) -> Result<ClaimedMessage, Box<dyn Error>> {
    Ok(ClaimedMessage::new(
        envelope.source().to_owned(),
        envelope.id().to_owned(),
        envelope.message_type().to_owned(),
        transport_subject,
        bytes,
        ATTEMPT,
        LeaseGeneration::new(GENERATION)?,
    )?)
}

fn assert_safe_diagnostics(error: &MessageRoutingError, forbidden: &[&str]) {
    let mut current: &dyn Error = error;
    loop {
        let display = current.to_string();
        let debug = format!("{current:?}");
        for sentinel in forbidden {
            let numeric_bytes = format!("{:?}", sentinel.as_bytes());
            assert!(!display.contains(sentinel));
            assert!(!debug.contains(sentinel));
            assert!(!display.contains(&numeric_bytes));
            assert!(!debug.contains(&numeric_bytes));
        }
        let Some(source) = current.source() else {
            break;
        };
        current = source;
    }
}

#[test]
fn registered_command_revalidates_without_changing_stored_evidence() -> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    let envelope = COMMAND.build(metadata(COMMAND), &PAYLOAD_SENTINEL)?;
    let claim = stored_claim(&envelope, COMMAND.subject()?, envelope.to_json()?)?;
    let before = claim.clone();

    let decoded = claim.validated_envelope(&registry)?;

    assert_eq!(decoded, envelope);
    assert_eq!(decoded.payload::<String>()?, PAYLOAD_SENTINEL);
    assert_eq!(decoded.id(), claim.message_id());
    assert_eq!(decoded.source(), claim.message_source());
    assert_eq!(decoded.message_type(), claim.message_type());
    assert_eq!(
        registry.validate(&decoded)?.subject()?,
        claim.transport_subject()
    );
    assert_eq!(claim.envelope_bytes(), envelope.to_json()?);
    assert_eq!(claim.attempt(), ATTEMPT);
    assert_eq!(claim.lease_generation().get(), GENERATION);
    assert_eq!(claim, before);
    Ok(())
}

#[test]
fn raw_byte_limit_accepts_valid_padding_and_rejects_the_adjacent_byte_before_parsing()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    let envelope = COMMAND.build(metadata(COMMAND), &PAYLOAD_SENTINEL)?;
    let mut bytes = envelope.to_json()?;
    assert!(bytes.len() < MAX_PORTABLE_MESSAGE_BYTES);
    bytes.resize(MAX_PORTABLE_MESSAGE_BYTES, b' ');
    let claim = stored_claim(&envelope, COMMAND.subject()?, bytes.clone())?;
    assert_eq!(claim.validated_envelope(&registry)?, envelope);
    assert_eq!(claim.envelope_bytes(), bytes);

    bytes.push(b' ');
    // Invalid oversized content must hit the raw-size gate, not the JSON parser.
    for bytes in [bytes, vec![b'!'; MAX_PORTABLE_MESSAGE_BYTES + 1]] {
        let claim = stored_claim(&envelope, COMMAND.subject()?, bytes.clone())?;
        let error = claim
            .validated_envelope(&registry)
            .err()
            .ok_or("expected oversized stored input rejection")?;
        let expected = MessageContractError::EnvelopeTooLarge {
            actual_bytes: MAX_PORTABLE_MESSAGE_BYTES + 1,
            maximum_bytes: MAX_PORTABLE_MESSAGE_BYTES,
        };
        assert_eq!(error, MessageRoutingError::Envelope(expected.clone()));
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<MessageContractError>()),
            Some(&expected)
        );
        assert!(
            error
                .source()
                .is_some_and(|source| source.source().is_none())
        );
        assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL]);
        assert_eq!(claim.envelope_bytes(), bytes);
    }
    Ok(())
}

#[test]
fn malformed_json_preserves_safe_contract_coordinates_without_parser_text()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    let envelope = COMMAND.build(metadata(COMMAND), &PAYLOAD_SENTINEL)?;
    let bytes = format!("{{\"{PAYLOAD_SENTINEL}\": invalid}}").into_bytes();
    let claim = stored_claim(&envelope, COMMAND.subject()?, bytes.clone())?;

    let error = claim
        .validated_envelope(&registry)
        .err()
        .ok_or("expected malformed stored JSON rejection")?;

    assert!(matches!(
        error,
        MessageRoutingError::Envelope(MessageContractError::EnvelopeDecoding {
            line: 1,
            column,
        }) if column > 0
    ));
    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<MessageContractError>())
    );
    assert!(
        error
            .source()
            .is_some_and(|source| source.source().is_none())
    );
    assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL]);
    assert_eq!(claim.envelope_bytes(), bytes);
    Ok(())
}

#[test]
fn unsupported_major_version_is_not_coerced_to_a_registered_version() -> Result<(), Box<dyn Error>>
{
    let registry = MessageRegistry::new(&[COMMAND])?;
    let mut input = metadata(COMMAND);
    input.message_type = "com.edgeagent.research.request-received.v2".to_owned();
    input.data_schema = "urn:edgeagent:schema:request-received:v2".to_owned();
    let envelope = MessageEnvelope::from_payload(input, &PAYLOAD_SENTINEL)?;
    let claim = stored_claim(
        &envelope,
        "edgeagent.command.research.request-received.v2".to_owned(),
        envelope.to_json()?,
    )?;

    let error = claim
        .validated_envelope(&registry)
        .err()
        .ok_or("expected unregistered major version rejection")?;

    assert_eq!(error, MessageRoutingError::UnsupportedMessageType);
    assert_eq!(error.to_string(), "unsupported message type");
    assert!(error.source().is_none());
    assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL, envelope.message_type()]);
    Ok(())
}

#[test]
fn every_denormalized_column_is_compared_without_exposing_either_value()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    let envelope = COMMAND.build(metadata(COMMAND), &PAYLOAD_SENTINEL)?;
    let subject = COMMAND.subject()?;
    for (field, source, id, message_type, subject) in [
        (
            "id",
            envelope.source(),
            METADATA_SENTINEL,
            envelope.message_type(),
            subject.as_str(),
        ),
        (
            "source",
            METADATA_SENTINEL,
            envelope.id(),
            envelope.message_type(),
            subject.as_str(),
        ),
        (
            "type",
            envelope.source(),
            envelope.id(),
            METADATA_SENTINEL,
            subject.as_str(),
        ),
        (
            "transport_subject",
            envelope.source(),
            envelope.id(),
            envelope.message_type(),
            METADATA_SENTINEL,
        ),
    ] {
        let claim = ClaimedMessage::new(
            source.to_owned(),
            id.to_owned(),
            message_type.to_owned(),
            subject.to_owned(),
            envelope.to_json()?,
            ATTEMPT,
            LeaseGeneration::new(GENERATION)?,
        )?;
        let before = claim.clone();

        let error = claim
            .validated_envelope(&registry)
            .err()
            .ok_or("expected changed stored column rejection")?;

        assert_eq!(error, MessageRoutingError::ContractMismatch { field });
        assert_eq!(error.to_string(), format!("message {field} mismatch"));
        assert!(error.source().is_none());
        assert_safe_diagnostics(
            &error,
            &[
                PAYLOAD_SENTINEL,
                METADATA_SENTINEL,
                envelope.id(),
                envelope.source(),
                envelope.message_type(),
                &COMMAND.subject()?,
            ],
        );
        assert_eq!(claim, before);
    }
    Ok(())
}

#[test]
fn matching_stored_columns_do_not_bypass_registry_schema_or_partition_policy()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    for field in ["dataschema", "partitionkey"] {
        let mut input = metadata(COMMAND);
        if field == "dataschema" {
            input.data_schema = format!("urn:edgeagent:schema:{METADATA_SENTINEL}");
        } else {
            input.partition_key = format!("different/{METADATA_SENTINEL}");
        }
        let envelope = MessageEnvelope::from_payload(input, &PAYLOAD_SENTINEL)?;
        let claim = stored_claim(&envelope, COMMAND.subject()?, envelope.to_json()?)?;

        let error = claim
            .validated_envelope(&registry)
            .err()
            .ok_or("expected registered routing rejection")?;

        assert_eq!(error, MessageRoutingError::ContractMismatch { field });
        assert!(error.source().is_none());
        assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL, METADATA_SENTINEL]);
    }
    Ok(())
}

#[test]
fn registered_event_requires_the_authoritative_producer_even_when_columns_agree()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[EVENT])?;
    let mut input = metadata(EVENT);
    input.source = Component::Research.source_uri().to_owned();
    let valid = EVENT.build(input.clone(), &PAYLOAD_SENTINEL)?;
    let claim = stored_claim(&valid, EVENT.subject()?, valid.to_json()?)?;
    assert_eq!(claim.validated_envelope(&registry)?, valid);

    input.source = format!("urn:edgeagent:producer:{METADATA_SENTINEL}");
    let wrong_producer = MessageEnvelope::from_payload(input, &PAYLOAD_SENTINEL)?;
    let claim = stored_claim(&wrong_producer, EVENT.subject()?, wrong_producer.to_json()?)?;
    let error = claim
        .validated_envelope(&registry)
        .err()
        .ok_or("expected unauthorized event producer rejection")?;
    assert_eq!(
        error,
        MessageRoutingError::ContractMismatch { field: "source" }
    );
    assert!(error.source().is_none());
    assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL, METADATA_SENTINEL]);
    Ok(())
}

#[test]
fn invalid_wire_metadata_and_missing_attributes_preserve_safe_decoding_categories()
-> Result<(), Box<dyn Error>> {
    let registry = MessageRegistry::new(&[COMMAND])?;
    let envelope = COMMAND.build(metadata(COMMAND), &PAYLOAD_SENTINEL)?;
    let bytes = String::from_utf8(envelope.to_json()?)?
        .replacen(
            envelope.message_type(),
            &format!("invalid type {METADATA_SENTINEL}"),
            1,
        )
        .into_bytes();
    let claim = stored_claim(&envelope, COMMAND.subject()?, bytes)?;
    let error = claim
        .validated_envelope(&registry)
        .err()
        .ok_or("expected invalid type rejection")?;
    assert!(matches!(
        error,
        MessageRoutingError::Envelope(MessageContractError::InvalidMetadata { field: "type", .. })
    ));
    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<MessageContractError>())
    );
    assert!(
        error
            .source()
            .is_some_and(|source| source.source().is_none())
    );
    assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL, METADATA_SENTINEL]);

    let bytes = format!("{{\"data\":\"{PAYLOAD_SENTINEL}\"}}").into_bytes();
    let claim = stored_claim(&envelope, COMMAND.subject()?, bytes)?;
    let error = claim
        .validated_envelope(&registry)
        .err()
        .ok_or("expected missing attribute rejection")?;
    assert_eq!(
        error,
        MessageRoutingError::Envelope(MessageContractError::EnvelopeDecoding {
            line: 0,
            column: 0
        })
    );
    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<MessageContractError>())
    );
    assert!(
        error
            .source()
            .is_some_and(|source| source.source().is_none())
    );
    assert_safe_diagnostics(&error, &[PAYLOAD_SENTINEL]);
    Ok(())
}
