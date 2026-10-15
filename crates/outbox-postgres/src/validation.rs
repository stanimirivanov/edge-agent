//! Bounded adapter inputs and PostgreSQL duration conversion.

use crate::OutboxError;
use std::time::Duration;

pub(super) const MAX_BATCH_SIZE: u16 = 1_000;
pub(super) const MAX_LEASE_OWNER_BYTES: usize = 128;
pub(super) const MAX_FAILURE_CODE_BYTES: usize = 64;
pub(super) const MAX_MESSAGE_SOURCE_BYTES: usize = 512;
pub(super) const MAX_IDENTIFIER_BYTES: usize = 128;
pub(super) const MAX_OPERATOR_ID_BYTES: usize = 256;

pub(super) fn validate_token(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), OutboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(OutboxError::invalid_argument(reason));
    }
    Ok(())
}

pub(super) fn validate_batch_size(value: u16) -> Result<(), OutboxError> {
    if (1..=MAX_BATCH_SIZE).contains(&value) {
        Ok(())
    } else {
        Err(OutboxError::invalid_argument(
            "batch_size must be between 1 and 1000",
        ))
    }
}

pub(super) fn validate_lease_owner(value: &str) -> Result<(), OutboxError> {
    validate_token(
        value,
        MAX_LEASE_OWNER_BYTES,
        "lease_owner must contain 1 to 128 lowercase ASCII token bytes",
    )
}

pub(super) fn validate_identifier(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), OutboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Err(OutboxError::invalid_argument(reason))
    } else {
        Ok(())
    }
}

pub(super) fn validate_visible_ascii(
    value: &str,
    maximum_bytes: usize,
    reason: &'static str,
) -> Result<(), OutboxError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(OutboxError::invalid_argument(reason))
    } else {
        Ok(())
    }
}

pub(super) fn duration_milliseconds(value: Duration) -> Result<i64, OutboxError> {
    // PostgreSQL interval parameters use whole milliseconds. Preserve the
    // adapter's truncation policy, including immediate submillisecond retries.
    i64::try_from(value.as_millis())
        .map_err(|_| OutboxError::invalid_argument("duration exceeds PostgreSQL range"))
}
