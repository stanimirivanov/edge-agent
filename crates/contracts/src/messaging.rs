//! Validated, transport-neutral EdgeAgent message envelopes.

use crate::routing::MAX_PORTABLE_MESSAGE_BYTES;
use cloudevents::Event;
use cloudevents::event::{AttributesReader, Data, EventBuilder, EventBuilderV10, ExtensionValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;

mod error;
mod validation;

pub use self::error::MessageContractError;

use self::error::envelope_decoding;
use self::validation::{
    invalid, validate_event, validate_identifier, validate_message_type, validate_source,
    validate_trace_parent, validate_trace_state, validate_visible,
};

const JSON_CONTENT_TYPE: &str = "application/json";
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
    /// Optional W3C `tracestate` list, limited to 512 bytes and 32 members.
    /// Empty members and surrounding HTTP whitespace are permitted.
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
            validate_trace_state(trace_state)?;
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
    /// # Examples
    ///
    /// ```
    /// use edgeagent_contracts::{MessageEnvelope, MessageMetadata};
    ///
    /// let metadata = MessageMetadata {
    ///     id: "message-01".into(),
    ///     source: "urn:edgeagent:component:gateway".into(),
    ///     message_type: "com.edgeagent.research.request-received.v1".into(),
    ///     subject: "request/request-01".into(),
    ///     time: "2026-09-26T00:00:00Z".into(),
    ///     data_schema: "urn:edgeagent:schema:request-received:v1".into(),
    ///     correlation_id: "correlation-01".into(),
    ///     causation_id: "command-01".into(),
    ///     idempotency_key: "request-01".into(),
    ///     partition_key: "request/request-01".into(),
    ///     trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
    ///     trace_state: None,
    /// };
    /// let envelope = MessageEnvelope::from_payload(metadata, &serde_json::json!({"request_id": "request-01"}))?;
    /// assert_eq!(envelope.payload::<serde_json::Value>()?["request_id"], "request-01");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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
    /// The stored JSON tree is borrowed during deserialization rather than
    /// cloned in full for every typed decode.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload does not match the requested type.
    pub fn payload<T: DeserializeOwned>(&self) -> Result<T, MessageContractError> {
        let Some(Data::Json(value)) = self.event.data() else {
            return Err(invalid("data", "must contain a JSON value"));
        };
        T::deserialize(value).map_err(|_| MessageContractError::PayloadDecoding)
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

#[cfg(test)]
mod tests;
