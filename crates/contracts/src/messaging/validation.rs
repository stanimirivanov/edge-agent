//! Envelope and metadata validation shared by producer and decoder.

use super::{
    CAUSATION_ID, CORRELATION_ID, IDEMPOTENCY_KEY, JSON_CONTENT_TYPE, MAX_METADATA_LENGTH,
    MessageContractError, PARTITION_KEY, TRACE_PARENT, TRACE_STATE,
};
use crate::message_type::parse_message_type;
use cloudevents::Event;
use cloudevents::event::{AttributesReader, Data, ExtensionValue, SpecVersion};
use url::Url;

const MAX_IDENTIFIER_LENGTH: usize = 128;

pub(super) fn validate_event(event: &Event) -> Result<(), MessageContractError> {
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
        validate_trace_state(trace_state)?;
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

pub(super) fn validate_identifier(
    field: &'static str,
    value: &str,
) -> Result<(), MessageContractError> {
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

pub(super) fn validate_source(value: &str) -> Result<(), MessageContractError> {
    validate_visible("source", value, MAX_METADATA_LENGTH)?;
    Url::parse(value)
        .map(|_| ())
        .map_err(|_| invalid("source", "must be an absolute URI"))
}

pub(super) fn validate_visible(
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

pub(super) fn validate_message_type(value: &str) -> Result<(), MessageContractError> {
    validate_visible("type", value, MAX_METADATA_LENGTH)?;
    if parse_message_type(value).is_none() {
        return Err(invalid(
            "type",
            "must match com.edgeagent.<domain>.<name>.v<positive-major>",
        ));
    }
    Ok(())
}

pub(super) fn validate_trace_parent(value: &str) -> Result<(), MessageContractError> {
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

pub(super) fn validate_trace_state(value: &str) -> Result<(), MessageContractError> {
    if value.len() > MAX_METADATA_LENGTH {
        return Err(invalid(TRACE_STATE, "exceeds 512 bytes"));
    }

    let mut keys = Vec::new();
    for (index, member) in value.split(',').enumerate() {
        // The W3C grammar counts empty members and permits HTTP OWS around each member.
        if index >= 32 {
            return Err(invalid(TRACE_STATE, "exceeds 32 list members"));
        }
        let member = member.trim_matches([' ', '\t']);
        if member.is_empty() {
            continue;
        }
        let Some((key, entry_value)) = member.split_once('=') else {
            return Err(invalid(TRACE_STATE, "must contain W3C key=value entries"));
        };
        if !valid_trace_state_key(key) || !valid_trace_state_value(entry_value) {
            return Err(invalid(TRACE_STATE, "must contain W3C key=value entries"));
        }
        if keys.contains(&key) {
            return Err(invalid(TRACE_STATE, "contains a duplicate key"));
        }
        keys.push(key);
    }
    Ok(())
}

fn valid_trace_state_key(key: &str) -> bool {
    let valid_tail = |byte: &u8| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'-' | b'*' | b'/')
    };
    if let Some((tenant, system)) = key.split_once('@') {
        (1..=241).contains(&tenant.len())
            && (1..=14).contains(&system.len())
            && tenant
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && system
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_lowercase)
            && tenant.as_bytes()[1..].iter().all(valid_tail)
            && system.as_bytes()[1..].iter().all(valid_tail)
    } else {
        (1..=256).contains(&key.len())
            && key.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && key.as_bytes()[1..].iter().all(valid_tail)
    }
}

fn valid_trace_state_value(value: &str) -> bool {
    (1..=256).contains(&value.len())
        && value
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte) && !matches!(byte, b',' | b'='))
        && value.as_bytes().last().is_some_and(|byte| *byte != b' ')
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) const fn invalid(field: &'static str, reason: &'static str) -> MessageContractError {
    MessageContractError::InvalidMetadata { field, reason }
}
