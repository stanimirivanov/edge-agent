use super::*;
use async_nats::jetstream::context::PublishError;
use edgeagent_contracts::{Component, MAX_PORTABLE_MESSAGE_BYTES, MessageMetadata};
use serde_json::json;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn metadata() -> MessageMetadata {
    MessageMetadata {
        id: "message-01".to_owned(),
        source: Component::Gateway.source_uri().to_owned(),
        message_type: COMMAND.message_type().to_owned(),
        subject: "order/order-01".to_owned(),
        time: "2026-09-26T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema().to_owned(),
        correlation_id: "correlation-01".to_owned(),
        causation_id: "request-01".to_owned(),
        idempotency_key: "order-01".to_owned(),
        partition_key: "order/order-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

fn envelope() -> Result<edgeagent_contracts::MessageEnvelope, MessageRoutingError> {
    COMMAND.build(metadata(), &json!({"mode": "dry_run"}))
}

#[test]
fn prepared_publication_derives_subject_identity_and_structured_payload()
-> Result<(), Box<dyn Error>> {
    let envelope = envelope()?;
    let prepared = prepare_publish(COMMAND, &envelope)?;

    assert_eq!(
        prepared.subject,
        "edgeagent.command.execution.submit-dry-run-order.v1"
    );
    assert_eq!(
        prepared.message_id,
        "31:urn:edgeagent:component:gateway:message-01"
    );
    assert_eq!(prepared.payload, envelope.to_json()?);
    Ok(())
}

#[test]
fn prepared_publication_debug_omits_payload_bytes() {
    let payload = b"private-publication-sentinel-7391";
    let prepared = PreparedPublish {
        subject: "events.subject".to_owned(),
        message_id: "message-01".to_owned(),
        payload: payload.to_vec(),
    };

    let rendered = format!("{prepared:?}");

    assert!(!rendered.contains(&format!("{payload:?}")));
    assert!(!rendered.contains("private-publication-sentinel-7391"));
    assert!(rendered.contains("payload_bytes"));
    assert_eq!(prepared.payload, payload);
}

#[test]
fn invalid_envelope_fails_before_transport_publication() -> Result<(), Box<dyn Error>> {
    let event_definition = MessageDefinition::event(
        COMMAND.message_type(),
        COMMAND.data_schema(),
        Component::ExecutionSimulator,
        COMMAND.partition_prefix(),
        edgeagent_contracts::EventRetention::Workflow,
    );
    let envelope = envelope()?;

    let result = prepare_publish(event_definition, &envelope);

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
fn send_and_acknowledgement_failures_have_distinct_retry_semantics() {
    use NatsPublishErrorKind as Nats;
    use PublishErrorKind::{ConfirmationUnknown, Rejected, Unavailable};
    for (kind, send, acknowledgement) in [
        (Nats::StreamNotFound, Rejected, Rejected),
        (Nats::WrongLastMessageId, Rejected, Rejected),
        (Nats::WrongLastSequence, Rejected, Rejected),
        (Nats::MaxPayloadExceeded, Rejected, Rejected),
        (Nats::MaxAckPending, Unavailable, Rejected),
        (Nats::TimedOut, Unavailable, ConfirmationUnknown),
        (Nats::BrokenPipe, Unavailable, ConfirmationUnknown),
        (Nats::Other, Unavailable, ConfirmationUnknown),
    ] {
        assert_eq!(
            map_send_error(PublishError::new(kind)).kind(),
            send,
            "send {kind:?}"
        );
        assert_eq!(
            map_acknowledgement_error(PublishError::new(kind)).kind(),
            acknowledgement,
            "acknowledgement {kind:?}"
        );
    }
}

#[test]
fn undecodable_and_unclassified_server_replies_remain_unknown() -> Result<(), Box<dyn Error>> {
    let decoding = serde_json::from_slice::<serde_json::Value>(b"not JSON")
        .err()
        .ok_or("fixture must fail decoding")?;
    let error = map_acknowledgement_error(PublishError::with_source(
        NatsPublishErrorKind::Other,
        decoding,
    ));
    assert_eq!(error.kind(), PublishErrorKind::ConfirmationUnknown);
    assert!(
        error
            .source()
            .and_then(|source| source.source())
            .is_some_and(|source| source.is::<serde_json::Error>())
    );

    for code in [10077, 19999] {
        let cause: async_nats::jetstream::Error = serde_json::from_value(json!({
            "code": 500, "err_code": code, "description": "unclassified server failure"
        }))?;
        let error = map_acknowledgement_error(PublishError::with_source(
            NatsPublishErrorKind::Other,
            cause,
        ));
        assert_eq!(error.kind(), PublishErrorKind::ConfirmationUnknown);
    }
    Ok(())
}

#[test]
fn oversized_publication_is_rejected_by_contract_preparation() -> Result<(), Box<dyn Error>> {
    let envelope = edgeagent_contracts::MessageEnvelope::from_payload(
        metadata(),
        &json!({"padding": "x".repeat(MAX_PORTABLE_MESSAGE_BYTES)}),
    )?;
    assert!(matches!(
        prepare_publish(COMMAND, &envelope),
        Err(MessageRoutingError::EnvelopeTooLarge { .. })
    ));
    Ok(())
}

#[test]
fn typed_server_size_rejection_is_not_an_unknown_confirmation() -> Result<(), Box<dyn Error>> {
    let cause: async_nats::jetstream::Error = serde_json::from_value(json!({
        "code": 400,
        "err_code": 10054,
        "description": "private-server-rejection-sentinel"
    }))?;
    let error = map_acknowledgement_error(PublishError::with_source(
        NatsPublishErrorKind::Other,
        cause,
    ));
    assert_eq!(error.kind(), PublishErrorKind::Rejected);
    assert!(
        !error
            .to_string()
            .contains("private-server-rejection-sentinel")
    );
    assert!(!format!("{error:?}").contains("private-server-rejection-sentinel"));
    let sdk = error
        .source()
        .and_then(|cause| cause.downcast_ref::<PublishError>())
        .ok_or("SDK cause must remain available")?;
    assert!(
        sdk.source()
            .is_some_and(|cause| cause.is::<async_nats::jetstream::Error>())
    );
    Ok(())
}

#[test]
fn broker_duplicate_acknowledgement_is_portable() {
    let acknowledgement = PublishAck {
        duplicate: true,
        ..PublishAck::default()
    };

    assert_eq!(
        receipt(acknowledgement).disposition(),
        PublishDisposition::Duplicate
    );
}
