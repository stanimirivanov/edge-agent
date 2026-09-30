//! Bounds and token checks before adapter storage work.

use crate::InboxError;

pub(super) const MAX_CONSUMER_NAME_BYTES: usize = 128;
pub(super) const MAX_DELIVERY_KEY_BYTES: usize = 512;
pub(super) const MAX_TRANSPORT_SUBJECT_BYTES: usize = 512;
pub(super) const MAX_FAILURE_CODE_BYTES: usize = 64;
pub(super) const MAX_REPLAY_REQUEST_ID_BYTES: usize = 128;
pub(super) const MAX_OPERATOR_ID_BYTES: usize = 256;

pub(super) fn validate_consumer_name(value: &str) -> Result<(), InboxError> {
    if value.is_empty() || value.len() > MAX_CONSUMER_NAME_BYTES {
        return Err(InboxError::invalid_consumer_name(
            "consumer_name must contain 1 to 128 bytes",
        ));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(InboxError::invalid_consumer_name(
            "consumer_name must be a lowercase ASCII token",
        ));
    }
    Ok(())
}

pub(super) fn validate_visible_ascii(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(InboxError::invalid_quarantine_evidence(reason))
    } else {
        Ok(())
    }
}

pub(super) fn validate_token(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Err(InboxError::invalid_quarantine_evidence(reason))
    } else {
        Ok(())
    }
}

pub(super) fn validate_replay_identifier(value: &str) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > MAX_REPLAY_REQUEST_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Err(InboxError::invalid_replay_request(
            "replay_request_id must contain 1 to 128 portable identifier bytes",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_replay_actor(value: &str) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > MAX_OPERATOR_ID_BYTES
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(InboxError::invalid_replay_request(
            "requested_by must contain 1 to 256 visible ASCII bytes",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_replay_reason(value: &str) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > MAX_FAILURE_CODE_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Err(InboxError::invalid_replay_request(
            "replay reason must be a lowercase ASCII token of 1 to 64 bytes",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_replay_target(value: &str) -> Result<(), InboxError> {
    if value.is_empty()
        || value.len() > MAX_DELIVERY_KEY_BYTES
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(InboxError::invalid_replay_request(
            "delivery_key must contain 1 to 512 visible ASCII bytes",
        ))
    } else {
        Ok(())
    }
}
