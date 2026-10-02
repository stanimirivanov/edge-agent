//! Validated, transport-neutral EdgeAgent message envelopes.

use crate::message_type::parse_message_type;
use crate::routing::MAX_PORTABLE_MESSAGE_BYTES;
use cloudevents::Event;
use cloudevents::event::{
    AttributesReader, Data, EventBuilder, EventBuilderV10, ExtensionValue, SpecVersion,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};
use url::Url;

const JSON_CONTENT_TYPE: &str = "application/json";
const MAX_IDENTIFIER_LENGTH: usize = 128;
pub(crate) const MAX_METADATA_LENGTH: usize = 512;
const CORRELATION_ID: &str = "correlationid";
const CAUSATION_ID: &str = "causationid";
const IDEMPOTENCY_KEY: &str = "idempotencykey";
const PARTITION_KEY: &str = "partitionkey";
const TRACE_PARENT: &str = "traceparent";
const TRACE_STATE: &str = "tracestate";

/// Caller-supplied metadata for one deterministic EdgeAgent message.
///
/// The caller owns identifier and timestamp generation. This contract does not
/// read the wall clock or generate random identifiers, which keeps creation
/// deterministic and makes retries explicit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageMetadata {
    /// Identity unique within the message source.
    pub id: String,
    /// Absolute URI identifying the producing EdgeAgent component.
    pub source: String,
    /// Versioned type such as `com.edgeagent.research.artifact-published.v1`.
    pub message_type: String,
    /// Aggregate-relative subject of this message.
    pub subject: String,
    /// RFC 3339 time of the occurrence or command creation.
    pub time: String,
    /// Absolute URI identifying the immutable payload schema.
    pub data_schema: String,
    /// Identifier shared by one end-to-end workflow.
    pub correlation_id: String,
    /// Identifier of the message that directly caused this message.
    pub causation_id: String,
    /// Stable key used to reject conflicting command retries.
    pub idempotency_key: String,
    /// Aggregate key within which ordering matters.
    pub partition_key: String,
    /// W3C Trace Context `traceparent` value using version 00.
    pub trace_parent: String,
    /// Optional bounded W3C `tracestate` value.
    pub trace_state: Option<String>,
}

impl MessageMetadata {
    fn validate(&self) -> Result<(), MessageContractError> {
        validate_identifier("id", &self.id)?;
        validate_source(&self.source)?;
        validate_message_type(&self.message_type)?;
        validate_visible("dataschema", &self.data_schema, MAX_METADATA_LENGTH)?;
        validate_visible("subject", &self.subject, MAX_METADATA_LENGTH)?;
        validate_identifier(CORRELATION_ID, &self.correlation_id)?;
        validate_identifier(CAUSATION_ID, &self.causation_id)?;
        validate_identifier(IDEMPOTENCY_KEY, &self.idempotency_key)?;
        validate_visible(PARTITION_KEY, &self.partition_key, MAX_METADATA_LENGTH)?;
        validate_trace_parent(&self.trace_parent)?;
        if let Some(trace_state) = &self.trace_state {
            validate_visible(TRACE_STATE, trace_state, MAX_METADATA_LENGTH)?;
        }
        Ok(())
    }
}

/// A CloudEvents 1.0 structured JSON message that passed EdgeAgent validation.
/// Its `Debug` representation never traverses the payload data.
#[derive(Clone, Eq, PartialEq)]
pub struct MessageEnvelope {
    event: Event,
}

// The SDK parses dataschema into Url, which may shorten a path during
// normalization. Check supplied text before that conversion so the metadata
// byte limit cannot be bypassed with dot segments.
#[derive(Deserialize)]
struct RawEnvelopeMetadata {
    #[serde(rename = "type")]
    message_type: Option<String>,
    dataschema: Option<String>,
}

impl std::fmt::Debug for MessageEnvelope {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MessageEnvelope")
            .field("message_type", &self.message_type())
            .finish_non_exhaustive()
    }
}

impl MessageEnvelope {
    /// Build and validate an envelope around a serializable payload.
    ///
    /// # Errors
    ///
    /// Returns an error when metadata violates the EdgeAgent contract, the
    /// payload cannot be represented as JSON, or the CloudEvent cannot be built.
    pub fn from_payload<T: Serialize>(
        metadata: MessageMetadata,
        payload: &T,
    ) -> Result<Self, MessageContractError> {
        metadata.validate()?;
        let data =
            serde_json::to_value(payload).map_err(|_| MessageContractError::PayloadEncoding)?;

        let mut builder = EventBuilderV10::new()
            .id(metadata.id)
            .source(metadata.source)
            .ty(metadata.message_type)
            .subject(metadata.subject)
            .time(metadata.time)
            .extension(CORRELATION_ID, metadata.correlation_id)
            .extension(CAUSATION_ID, metadata.causation_id)
            .extension(IDEMPOTENCY_KEY, metadata.idempotency_key)
            .extension(PARTITION_KEY, metadata.partition_key)
            .extension(TRACE_PARENT, metadata.trace_parent)
            .data_with_schema(JSON_CONTENT_TYPE, metadata.data_schema, data);
        if let Some(trace_state) = metadata.trace_state {
            builder = builder.extension(TRACE_STATE, trace_state);
        }

        let event = builder
            .build()
            .map_err(|_| MessageContractError::CloudEvent)?;
        validate_event(&event)?;
        Ok(Self { event })
    }

    /// Decode and validate a CloudEvents structured JSON message.
    ///
    /// # Errors
    ///
    /// Returns an error for an oversized envelope before parsing, malformed
    /// JSON, unsupported CloudEvents versions, absent required attributes,
    /// invalid extensions, or non-JSON payloads.
    pub fn from_json(bytes: &[u8]) -> Result<Self, MessageContractError> {
        if bytes.len() > MAX_PORTABLE_MESSAGE_BYTES {
            return Err(MessageContractError::EnvelopeTooLarge {
                actual_bytes: bytes.len(),
                maximum_bytes: MAX_PORTABLE_MESSAGE_BYTES,
            });
        }
        let raw_metadata: RawEnvelopeMetadata =
            serde_json::from_slice(bytes).map_err(envelope_decoding)?;
        if let Some(message_type) = raw_metadata.message_type.as_deref() {
            validate_message_type(message_type)?;
        }
        if let Some(data_schema) = raw_metadata.dataschema.as_deref() {
            validate_visible("dataschema", data_schema, MAX_METADATA_LENGTH)?;
        }
        let event = serde_json::from_slice(bytes).map_err(envelope_decoding)?;
        validate_event(&event)?;
        Ok(Self { event })
    }

    /// Encode this validated envelope as CloudEvents structured JSON.
    ///
    /// # Errors
    ///
    /// Returns an error only if the SDK cannot serialize its validated event.
    pub fn to_json(&self) -> Result<Vec<u8>, MessageContractError> {
        let value = serde_json::to_value(&self.event)
            .map_err(|_| MessageContractError::EnvelopeEncoding)?;
        serde_json::to_vec(&value).map_err(|_| MessageContractError::EnvelopeEncoding)
    }

    /// Decode the JSON data field into a caller-owned payload type.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload does not match the requested type.
    pub fn payload<T: DeserializeOwned>(&self) -> Result<T, MessageContractError> {
        let Some(Data::Json(value)) = self.event.data() else {
            return Err(invalid("data", "must contain a JSON value"));
        };
        serde_json::from_value(value.clone()).map_err(|_| MessageContractError::PayloadDecoding)
    }

    /// Return the message identity.
    #[must_use]
    pub fn id(&self) -> &str {
        self.event.id()
    }

    /// Return the stable transport deduplication key for this CloudEvent.
    ///
    /// CloudEvents identity is the pair of `source` and `id`. The length prefix
    /// keeps the representation unambiguous without hashing away diagnostics.
    #[must_use]
    pub fn deduplication_key(&self) -> String {
        format!("{}:{}:{}", self.source().len(), self.source(), self.id())
    }

    /// Return the versioned command or event type.
    #[must_use]
    pub fn message_type(&self) -> &str {
        self.event.ty()
    }

    /// Return the absolute producer URI.
    #[must_use]
    pub fn source(&self) -> &str {
        self.event.source()
    }

    /// Return the message subject.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.event.subject()
    }

    /// Return the immutable payload schema URI.
    #[must_use]
    pub fn data_schema(&self) -> Option<&str> {
        self.event.dataschema().map(url::Url::as_str)
    }

    /// Return the declared aggregate ordering key.
    #[must_use]
    pub fn partition_key(&self) -> Option<&str> {
        self.extension(PARTITION_KEY)
    }

    /// Return an extension value when it is present and string-valued.
    #[must_use]
    pub fn extension(&self, name: &str) -> Option<&str> {
        match self.event.extension(name) {
            Some(ExtensionValue::String(value)) => Some(value),
            _ => None,
        }
    }
}

/// Validation or encoding failure at the EdgeAgent message boundary.
///
/// Serde and CloudEvents diagnostic text may include caller-controlled data.
/// This error retains only stable categories, safe field names, byte counts,
/// and parser coordinates; it does not expose the underlying diagnostic as a
/// source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageContractError {
    /// One required metadata value violates a stable invariant.
    InvalidMetadata {
        /// CloudEvents or EdgeAgent extension field name.
        field: &'static str,
        /// Stable, non-sensitive explanation.
        reason: &'static str,
    },
    /// Payload-to-JSON serialization failed; serializer text is withheld.
    PayloadEncoding,
    /// JSON-to-payload deserialization failed; decoder text is withheld.
    PayloadDecoding,
    /// CloudEvents construction failed; SDK text is withheld.
    CloudEvent,
    /// Structured JSON serialization failed; serializer text is withheld.
    EnvelopeEncoding,
    /// Structured JSON parsing failed; only safe parser coordinates are retained.
    EnvelopeDecoding {
        /// Parser line, when available (zero for semantic errors).
        line: usize,
        /// Parser column, when available (zero for semantic errors).
        column: usize,
    },
    /// Raw structured envelope exceeds the portable byte limit.
    EnvelopeTooLarge {
        /// Actual raw input length in bytes.
        actual_bytes: usize,
        /// Maximum permitted raw input length in bytes.
        maximum_bytes: usize,
    },
}

impl Display for MessageContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetadata { field, reason } => {
                write!(formatter, "invalid {field}: {reason}")
            }
            Self::PayloadEncoding => formatter.write_str("cannot encode payload"),
            Self::PayloadDecoding => formatter.write_str("cannot decode payload"),
            Self::CloudEvent => formatter.write_str("cannot build CloudEvent"),
            Self::EnvelopeEncoding => formatter.write_str("cannot encode CloudEvent envelope"),
            Self::EnvelopeDecoding { line, column } => write!(
                formatter,
                "cannot decode CloudEvent envelope at line {line}, column {column}"
            ),
            Self::EnvelopeTooLarge {
                actual_bytes,
                maximum_bytes,
            } => write!(
                formatter,
                "CloudEvent envelope is {actual_bytes} bytes; maximum is {maximum_bytes} bytes"
            ),
        }
    }
}

impl Error for MessageContractError {}

fn envelope_decoding(error: serde_json::Error) -> MessageContractError {
    MessageContractError::EnvelopeDecoding {
        line: error.line(),
        column: error.column(),
    }
}

fn validate_event(event: &Event) -> Result<(), MessageContractError> {
    if event.specversion() != SpecVersion::V10 {
        return Err(invalid("specversion", "must be CloudEvents 1.0"));
    }
    validate_identifier("id", event.id())?;
    validate_source(event.source())?;
    validate_message_type(event.ty())?;
    validate_visible(
        "subject",
        event
            .subject()
            .ok_or_else(|| invalid("subject", "is required"))?,
        MAX_METADATA_LENGTH,
    )?;
    if event.time().is_none() {
        return Err(invalid("time", "is required"));
    }
    if event.datacontenttype() != Some(JSON_CONTENT_TYPE) {
        return Err(invalid("datacontenttype", "must be application/json"));
    }
    validate_visible(
        "dataschema",
        event
            .dataschema()
            .ok_or_else(|| invalid("dataschema", "is required"))?
            .as_str(),
        MAX_METADATA_LENGTH,
    )?;
    if !matches!(event.data(), Some(Data::Json(_))) {
        return Err(invalid("data", "must contain a JSON value"));
    }

    validate_identifier(CORRELATION_ID, required_extension(event, CORRELATION_ID)?)?;
    validate_identifier(CAUSATION_ID, required_extension(event, CAUSATION_ID)?)?;
    validate_identifier(IDEMPOTENCY_KEY, required_extension(event, IDEMPOTENCY_KEY)?)?;
    validate_visible(
        PARTITION_KEY,
        required_extension(event, PARTITION_KEY)?,
        MAX_METADATA_LENGTH,
    )?;
    validate_trace_parent(required_extension(event, TRACE_PARENT)?)?;
    if let Some(trace_state) = optional_string_extension(event, TRACE_STATE)? {
        validate_visible(TRACE_STATE, trace_state, MAX_METADATA_LENGTH)?;
    }
    Ok(())
}

fn required_extension<'event>(
    event: &'event Event,
    name: &'static str,
) -> Result<&'event str, MessageContractError> {
    optional_string_extension(event, name)?.ok_or_else(|| invalid(name, "is required"))
}

fn optional_string_extension<'event>(
    event: &'event Event,
    name: &'static str,
) -> Result<Option<&'event str>, MessageContractError> {
    match event.extension(name) {
        Some(ExtensionValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(invalid(name, "must be a string")),
        None => Ok(None),
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), MessageContractError> {
    if value.is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > MAX_IDENTIFIER_LENGTH {
        return Err(invalid(field, "exceeds 128 bytes"));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(invalid(
            field,
            "must use ASCII letters, digits, hyphen, underscore, dot, or colon",
        ));
    }
    Ok(())
}

fn validate_source(value: &str) -> Result<(), MessageContractError> {
    validate_visible("source", value, MAX_METADATA_LENGTH)?;
    Url::parse(value)
        .map(|_| ())
        .map_err(|_| invalid("source", "must be an absolute URI"))
}

fn validate_visible(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), MessageContractError> {
    if value.is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > maximum {
        return Err(invalid(field, "exceeds its byte limit"));
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(invalid(field, "must contain visible ASCII without spaces"));
    }
    Ok(())
}

fn validate_message_type(value: &str) -> Result<(), MessageContractError> {
    validate_visible("type", value, MAX_METADATA_LENGTH)?;
    if parse_message_type(value).is_none() {
        return Err(invalid(
            "type",
            "must match com.edgeagent.<domain>.<name>.v<positive-major>",
        ));
    }
    Ok(())
}

fn validate_trace_parent(value: &str) -> Result<(), MessageContractError> {
    let segments = value.split('-').collect::<Vec<_>>();
    if segments.len() != 4
        || segments[0] != "00"
        || !is_lower_hex(segments[1], 32)
        || !is_lower_hex(segments[2], 16)
        || !is_lower_hex(segments[3], 2)
        || segments[1].bytes().all(|byte| byte == b'0')
        || segments[2].bytes().all(|byte| byte == b'0')
    {
        return Err(invalid(
            TRACE_PARENT,
            "must be a valid non-zero W3C version 00 trace context",
        ));
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

const fn invalid(field: &'static str, reason: &'static str) -> MessageContractError {
    MessageContractError::InvalidMetadata { field, reason }
}

#[cfg(test)]
mod tests {
    use super::{MAX_METADATA_LENGTH, MessageContractError, MessageEnvelope, MessageMetadata};
    use crate::MAX_PORTABLE_MESSAGE_BYTES;
    use cloudevents::event::{EventBuilder, EventBuilderV03};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Value, json};
    use std::error::Error;

    const FIXTURE: &[u8] = include_bytes!("../fixtures/v1/research-request-received.json");

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
        assert_eq!(encoded, repeated);
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
    fn decoder_rejects_overlong_schema_even_when_uri_normalizes_shorter()
    -> Result<(), Box<dyn Error>> {
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
}
