use super::{MAX_METADATA_LENGTH, MessageContractError, MessageEnvelope, MessageMetadata};
use crate::MAX_PORTABLE_MESSAGE_BYTES;
use cloudevents::event::{EventBuilder, EventBuilderV03};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use std::error::Error;
use std::hint::black_box;
use std::time::Instant;

const FIXTURE: &[u8] = include_bytes!("../../fixtures/v1/research-request-received.json");

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ResearchRequestReceived {
    request_id: String,
}

const SECRET_SENTINEL: &str = "private-payload-sentinel-7391";

struct RejectingPayload;

impl Serialize for RejectingPayload {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(<S::Error as serde::ser::Error>::custom(SECRET_SENTINEL))
    }
}

#[derive(Debug)]
struct RejectingDecode;

impl<'de> Deserialize<'de> for RejectingDecode {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(<D::Error as serde::de::Error>::custom(SECRET_SENTINEL))
    }
}

fn metadata() -> MessageMetadata {
    MessageMetadata {
        id: "message-01".to_owned(),
        source: "urn:edgeagent:component:gateway".to_owned(),
        message_type: "com.edgeagent.research.request-received.v1".to_owned(),
        subject: "research-request/request-01".to_owned(),
        time: "2026-09-26T00:00:00Z".to_owned(),
        data_schema: "urn:edgeagent:schema:research-request-received:v1".to_owned(),
        correlation_id: "correlation-01".to_owned(),
        causation_id: "command-01".to_owned(),
        idempotency_key: "research-request-01".to_owned(),
        partition_key: "research-request/request-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: Some("vendor=value".to_owned()),
    }
}

#[test]
fn fixture_round_trips_and_decodes_typed_payload() -> Result<(), Box<dyn Error>> {
    let envelope = MessageEnvelope::from_json(FIXTURE)?;
    let payload: ResearchRequestReceived = envelope.payload()?;
    let encoded = envelope.to_json()?;
    let expected: Value = serde_json::from_slice(FIXTURE)?;
    let actual: Value = serde_json::from_slice(&encoded)?;

    assert_eq!(payload.request_id, "request-01");
    assert_eq!(actual, expected);
    assert_eq!(encoded, serde_json::to_vec(&expected)?);
    assert_eq!(envelope.id(), "message-01");
    assert_eq!(
        envelope.deduplication_key(),
        "31:urn:edgeagent:component:gateway:message-01"
    );
    assert_eq!(envelope.extension("correlationid"), Some("correlation-01"));
    Ok(())
}

#[test]
fn payload_builder_matches_golden_fixture() -> Result<(), Box<dyn Error>> {
    let payload = ResearchRequestReceived {
        request_id: "request-01".to_owned(),
    };
    let envelope = MessageEnvelope::from_payload(metadata(), &payload)?;
    let encoded = envelope.to_json()?;
    let repeated = MessageEnvelope::from_payload(metadata(), &payload)?.to_json()?;
    let expected: Value = serde_json::from_slice(FIXTURE)?;
    let actual: Value = serde_json::from_slice(&encoded)?;

    assert_eq!(actual, expected);
    assert_eq!(encoded, serde_json::to_vec(&expected)?);
    assert_eq!(encoded, repeated);
    Ok(())
}

#[test]
#[ignore = "manual, environment-dependent throughput probe"]
fn payload_decode_throughput_probe() -> Result<(), Box<dyn Error>> {
    const ROUNDS: usize = 300;
    let payload = (0..40_000_u32).collect::<Vec<_>>();
    let envelope = MessageEnvelope::from_payload(metadata(), &payload)?;
    let message_bytes = envelope.to_json()?.len();
    assert!((MAX_PORTABLE_MESSAGE_BYTES / 2..=MAX_PORTABLE_MESSAGE_BYTES).contains(&message_bytes));
    let Some(cloudevents::event::Data::Json(value)) = envelope.event.data() else {
        return Err("benchmark payload must be JSON".into());
    };

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let decoded: Vec<u32> = serde_json::from_value(black_box(value.clone()))?;
        black_box(decoded);
    }
    let cloned = start.elapsed();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let decoded = black_box(&envelope).payload::<Vec<u32>>()?;
        black_box(decoded);
    }
    let borrowed = start.elapsed();

    eprintln!(
        "payload decode, {message_bytes} bytes, {ROUNDS} rounds: cloned={cloned:?}, borrowed={borrowed:?}"
    );
    Ok(())
}

#[test]
fn envelope_debug_omits_json_payload() -> Result<(), Box<dyn Error>> {
    const SENTINEL: &str = "licensed-payload-sentinel-7391";
    let payload = json!({"private_content": SENTINEL});
    let envelope = MessageEnvelope::from_payload(metadata(), &payload)?;

    let rendered = format!("{envelope:?}");

    assert!(rendered.contains("MessageEnvelope"));
    assert!(!rendered.contains(SENTINEL));
    assert_eq!(envelope.payload::<Value>()?, payload);
    Ok(())
}

#[test]
fn contract_errors_do_not_expose_serializer_or_decoder_text() -> Result<(), Box<dyn Error>> {
    let Err(encoding) = MessageEnvelope::from_payload(metadata(), &RejectingPayload) else {
        return Err("custom serializer must fail".into());
    };
    let envelope = MessageEnvelope::from_payload(metadata(), &json!({}))?;
    let Err(decoding) = envelope.payload::<RejectingDecode>() else {
        return Err("custom deserializer must fail".into());
    };

    for error in [encoding, decoding] {
        assert!(!error.to_string().contains(SECRET_SENTINEL));
        assert!(!format!("{error:?}").contains(SECRET_SENTINEL));
    }
    Ok(())
}

#[test]
fn malformed_envelope_error_exposes_only_parser_coordinates() -> Result<(), Box<dyn Error>> {
    let input = format!("{{\"type\": {SECRET_SENTINEL}}}");
    let Err(error) = MessageEnvelope::from_json(input.as_bytes()) else {
        return Err("malformed JSON must fail".into());
    };

    assert!(matches!(
        error,
        MessageContractError::EnvelopeDecoding { line: 1, .. }
    ));
    assert!(!error.to_string().contains(SECRET_SENTINEL));
    assert!(!format!("{error:?}").contains(SECRET_SENTINEL));
    Ok(())
}

#[test]
fn invalid_message_type_is_rejected_before_serialization() {
    let mut invalid_metadata = metadata();
    invalid_metadata.message_type = "research.request.received".to_owned();

    let result = MessageEnvelope::from_payload(invalid_metadata, &json!({}));

    assert!(matches!(
        result,
        Err(MessageContractError::InvalidMetadata { field: "type", .. })
    ));
}

#[test]
fn raw_envelope_limit_precedes_json_decoding() -> Result<(), Box<dyn Error>> {
    let mut at_limit = FIXTURE.to_vec();
    at_limit.resize(MAX_PORTABLE_MESSAGE_BYTES, b' ');
    assert!(MessageEnvelope::from_json(&at_limit).is_ok());

    at_limit.push(b' ');
    assert_eq!(
        MessageEnvelope::from_json(&at_limit),
        Err(MessageContractError::EnvelopeTooLarge {
            actual_bytes: MAX_PORTABLE_MESSAGE_BYTES + 1,
            maximum_bytes: MAX_PORTABLE_MESSAGE_BYTES,
        })
    );
    assert!(matches!(
        MessageEnvelope::from_json(&vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES + 1]),
        Err(MessageContractError::EnvelopeTooLarge { .. })
    ));
    assert!(matches!(
        MessageEnvelope::from_json(b"not json"),
        Err(MessageContractError::EnvelopeDecoding { .. })
    ));
    Ok(())
}

#[test]
fn message_type_limit_applies_to_producer_and_decoder() -> Result<(), Box<dyn Error>> {
    let prefix = "com.edgeagent.research.";
    let suffix = ".v1";
    let name = "a".repeat(MAX_METADATA_LENGTH - prefix.len() - suffix.len());
    let at_limit = format!("{prefix}{name}{suffix}");
    let over_limit = format!("{prefix}{name}a{suffix}");

    for (value, accepted) in [(at_limit, true), (over_limit, false)] {
        let mut candidate = metadata();
        candidate.message_type.clone_from(&value);
        let producer = MessageEnvelope::from_payload(candidate, &json!({}));
        assert_eq!(producer.is_ok(), accepted);
        if !accepted {
            assert!(matches!(
                producer,
                Err(MessageContractError::InvalidMetadata { field: "type", .. })
            ));
        }

        let mut wire: Value = serde_json::from_slice(FIXTURE)?;
        wire["type"] = json!(value);
        let decoder = MessageEnvelope::from_json(&serde_json::to_vec(&wire)?);
        assert_eq!(decoder.is_ok(), accepted);
        if !accepted {
            assert!(matches!(
                decoder,
                Err(MessageContractError::InvalidMetadata { field: "type", .. })
            ));
        }
    }
    Ok(())
}

#[test]
fn schema_limit_applies_to_producer_and_decoder() -> Result<(), Box<dyn Error>> {
    let prefix = "urn:edgeagent:schema:";
    let name = "a".repeat(MAX_METADATA_LENGTH - prefix.len());
    let at_limit = format!("{prefix}{name}");
    let over_limit = format!("{prefix}{name}a");

    for (value, accepted) in [(at_limit, true), (over_limit, false)] {
        let mut candidate = metadata();
        candidate.data_schema.clone_from(&value);
        let producer = MessageEnvelope::from_payload(candidate, &json!({}));
        assert_eq!(producer.is_ok(), accepted);
        if !accepted {
            assert!(matches!(
                producer,
                Err(MessageContractError::InvalidMetadata {
                    field: "dataschema",
                    ..
                })
            ));
        }

        let mut wire: Value = serde_json::from_slice(FIXTURE)?;
        wire["dataschema"] = json!(value);
        let decoder = MessageEnvelope::from_json(&serde_json::to_vec(&wire)?);
        assert_eq!(decoder.is_ok(), accepted);
        if !accepted {
            assert!(matches!(
                decoder,
                Err(MessageContractError::InvalidMetadata {
                    field: "dataschema",
                    ..
                })
            ));
        }
    }
    Ok(())
}

#[test]
fn decoder_rejects_overlong_schema_even_when_uri_normalizes_shorter() -> Result<(), Box<dyn Error>>
{
    let schema = format!("https://example.invalid/{}", "a/../".repeat(110));
    assert!(schema.len() > MAX_METADATA_LENGTH);
    let mut wire: Value = serde_json::from_slice(FIXTURE)?;
    wire["dataschema"] = json!(schema);

    assert!(matches!(
        MessageEnvelope::from_json(&serde_json::to_vec(&wire)?),
        Err(MessageContractError::InvalidMetadata {
            field: "dataschema",
            ..
        })
    ));
    Ok(())
}

#[test]
fn zero_trace_identifier_is_rejected() {
    let mut invalid_metadata = metadata();
    invalid_metadata.trace_parent =
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01".to_owned();

    let result = MessageEnvelope::from_payload(invalid_metadata, &json!({}));

    assert!(matches!(
        result,
        Err(MessageContractError::InvalidMetadata {
            field: "traceparent",
            ..
        })
    ));
}

#[test]
fn w3c_tracestate_is_accepted_by_producer_and_decoder() -> Result<(), Box<dyn Error>> {
    let thirty_two_members = (0..32)
        .map(|index| format!("vendor{index}=value"))
        .collect::<Vec<_>>()
        .join(",");
    let max_key = format!("{}=x", "a".repeat(256));
    let max_value = format!("a={}", "x".repeat(256));
    let max_state = format!("a={},b={}", "x".repeat(254), "y".repeat(253));
    assert_eq!(max_state.len(), MAX_METADATA_LENGTH);
    for state in [
        "",
        " \t ",
        "vendor=one, other=two",
        "\tvendor=one two\t,,\tother=three\t",
        "1tenant@system=value",
        &thirty_two_members,
        &max_key,
        &max_value,
        &max_state,
    ] {
        let mut candidate = metadata();
        candidate.trace_state = Some(state.to_owned());
        let produced = MessageEnvelope::from_payload(candidate, &json!({}))?;
        assert_eq!(produced.extension("tracestate"), Some(state));

        let mut wire: Value = serde_json::from_slice(FIXTURE)?;
        wire["tracestate"] = json!(state);
        let decoded = MessageEnvelope::from_json(&serde_json::to_vec(&wire)?)?;
        assert_eq!(decoded.extension("tracestate"), Some(state));
    }
    Ok(())
}

#[test]
fn malformed_tracestate_is_rejected_by_producer_and_decoder() -> Result<(), Box<dyn Error>> {
    let thirty_three_members = (0..33)
        .map(|index| format!("vendor{index}=value"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong_key = format!("{}=x", "a".repeat(257));
    let overlong_value = format!("a={}", "x".repeat(257));
    let overlong_state = format!("a={}", "x".repeat(MAX_METADATA_LENGTH));
    let thirty_three_empty_members = ",".repeat(32);
    for state in [
        "vendor",
        "Vendor=value",
        "vendor=",
        "vendor=value=more",
        "vendor=one,vendor=two",
        "vendor.one=value",
        "tenant@1system=value",
        "a@b@c=value",
        "vendor=one\nmore",
        &thirty_three_members,
        &thirty_three_empty_members,
        &overlong_key,
        &overlong_value,
        &overlong_state,
    ] {
        let mut candidate = metadata();
        candidate.trace_state = Some(state.to_owned());
        assert!(matches!(
            MessageEnvelope::from_payload(candidate, &json!({})),
            Err(MessageContractError::InvalidMetadata {
                field: "tracestate",
                ..
            })
        ));

        let mut wire: Value = serde_json::from_slice(FIXTURE)?;
        wire["tracestate"] = json!(state);
        assert!(matches!(
            MessageEnvelope::from_json(&serde_json::to_vec(&wire)?),
            Err(MessageContractError::InvalidMetadata {
                field: "tracestate",
                ..
            })
        ));
    }
    Ok(())
}

#[test]
fn relative_source_is_rejected() {
    let mut invalid_metadata = metadata();
    invalid_metadata.source = "/gateway".to_owned();

    let result = MessageEnvelope::from_payload(invalid_metadata, &json!({}));

    assert!(matches!(
        result,
        Err(MessageContractError::InvalidMetadata {
            field: "source",
            ..
        })
    ));
}

#[test]
fn cloud_events_v03_is_rejected() -> Result<(), Box<dyn Error>> {
    let event = EventBuilderV03::new()
        .id("message-01")
        .source("urn:edgeagent:component:gateway")
        .ty("com.edgeagent.research.request-received.v1")
        .build()?;
    let encoded = serde_json::to_vec(&event)?;

    let result = MessageEnvelope::from_json(&encoded);

    assert!(matches!(
        result,
        Err(MessageContractError::InvalidMetadata {
            field: "specversion",
            ..
        })
    ));
    Ok(())
}

#[test]
fn missing_idempotency_key_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut value: Value = serde_json::from_slice(FIXTURE)?;
    let Value::Object(object) = &mut value else {
        return Err("fixture must be a JSON object".into());
    };
    object.remove("idempotencykey");
    let encoded = serde_json::to_vec(&value)?;

    let result = MessageEnvelope::from_json(&encoded);

    assert!(matches!(
        result,
        Err(MessageContractError::InvalidMetadata {
            field: "idempotencykey",
            ..
        })
    ));
    Ok(())
}

#[test]
fn unknown_extension_survives_a_round_trip() -> Result<(), Box<dyn Error>> {
    let mut value: Value = serde_json::from_slice(FIXTURE)?;
    let Value::Object(object) = &mut value else {
        return Err("fixture must be a JSON object".into());
    };
    object.insert("profilehint".to_owned(), json!("portable"));

    let envelope = MessageEnvelope::from_json(&serde_json::to_vec(&value)?)?;
    let encoded: Value = serde_json::from_slice(&envelope.to_json()?)?;

    assert_eq!(encoded.get("profilehint"), Some(&json!("portable")));
    Ok(())
}
