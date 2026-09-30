//! Bounded adapter inputs and PostgreSQL duration conversion.

use crate::OutboxError;
use std::time::Duration;

pub(super) const MAX_BATCH_SIZE: u16 = 1_000;
pub(super) const MAX_LEASE_DURATION: Duration = Duration::from_secs(15 * 60);
pub(super) const MAX_RETRY_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
pub(super) const MAX_LEASE_OWNER_BYTES: usize = 128;
pub(super) const MAX_FAILURE_CODE_BYTES: usize = 64;
pub(super) const MAX_MESSAGE_SOURCE_BYTES: usize = 512;
pub(super) const MAX_IDENTIFIER_BYTES: usize = 128;
pub(super) const MAX_OPERATOR_ID_BYTES: usize = 256;

pub(super) fn validate_token(
    field: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), OutboxError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(OutboxError::invalid_argument(match field {
            "lease_owner" => "lease_owner must contain 1 to 128 bytes",
            "replay_reason" => "replay reason must contain 1 to 64 bytes",
            _ => "failure_code or quarantine_reason must contain 1 to 64 bytes",
        }));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(OutboxError::invalid_argument(match field {
            "lease_owner" => "lease_owner must be a lowercase ASCII token",
            "replay_reason" => "replay reason must be a lowercase ASCII token",
            _ => "failure_code or quarantine_reason must be a lowercase ASCII token",
        }));
    }
    Ok(())
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

pub(super) fn duration_milliseconds(
    field: &'static str,
    value: Duration,
    minimum: Duration,
    maximum: Duration,
) -> Result<i64, OutboxError> {
    if value < minimum || value > maximum {
        return Err(OutboxError::invalid_argument(match field {
            "lease_duration" => "lease_duration must be between 1 millisecond and 15 minutes",
            _ => "retry_after must not exceed 24 hours",
        }));
    }
    i64::try_from(value.as_millis())
        .map_err(|_| OutboxError::invalid_argument("duration exceeds PostgreSQL range"))
}
