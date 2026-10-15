//! Audited authorization and idempotent release of quarantined messages.

use crate::validation::{
    MAX_FAILURE_CODE_BYTES, MAX_IDENTIFIER_BYTES, MAX_MESSAGE_SOURCE_BYTES, MAX_OPERATOR_ID_BYTES,
    validate_identifier, validate_token, validate_visible_ascii,
};
use crate::{OutboxError, PostgresOutbox};
use sqlx::{Postgres, Transaction};

/// Idempotent result of an authorized outbound replay request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayDisposition {
    /// Audit evidence was appended and the quarantined record became claimable.
    Released,
    /// This exact replay request was already applied.
    AlreadyRequested,
}

/// Bounded authorization evidence supplied by the operator control plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayRequest<'request> {
    request_id: &'request str,
    requested_by: &'request str,
    reason: &'request str,
}

impl<'request> ReplayRequest<'request> {
    /// Construct validated audit evidence for one replay authorization.
    ///
    /// The caller remains responsible for authenticating and authorizing the
    /// operator before constructing this value.
    ///
    /// # Errors
    ///
    /// Returns `InvalidArgument` when an identifier or reason violates its bound.
    pub fn new(
        request_id: &'request str,
        requested_by: &'request str,
        reason: &'request str,
    ) -> Result<Self, OutboxError> {
        validate_identifier(
            request_id,
            MAX_IDENTIFIER_BYTES,
            "replay_request_id must contain 1 to 128 portable identifier bytes",
        )?;
        validate_visible_ascii(
            requested_by,
            MAX_OPERATOR_ID_BYTES,
            "requested_by must contain 1 to 256 visible ASCII bytes",
        )?;
        validate_token(
            reason,
            MAX_FAILURE_CODE_BYTES,
            "replay reason must contain 1 to 64 lowercase ASCII token bytes",
        )?;
        Ok(Self {
            request_id,
            requested_by,
            reason,
        })
    }
}

impl PostgresOutbox {
    /// Append an authorized replay decision and release a quarantined record.
    ///
    /// The immutable envelope and identity are preserved. A successful release
    /// resets the publication-attempt budget, while the audit row retains the
    /// prior attempt count, quarantine reason, actor, and database timestamps.
    /// Repeating the same request is idempotent, even after later state changes.
    ///
    /// # Errors
    ///
    /// Returns an invalid argument, conflicting request identity, non-quarantined
    /// target, storage, or storage-invariant failure.
    pub async fn replay_quarantined(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        message_source: &str,
        message_id: &str,
        request: ReplayRequest<'_>,
    ) -> Result<ReplayDisposition, OutboxError> {
        validate_target(message_source, message_id)?;
        let rows = sqlx::query_file!(
            "queries/replay_quarantined.sql",
            message_source,
            message_id,
            request.request_id,
            request.requested_by,
            request.reason,
        )
        .fetch_all(&mut **transaction)
        .await
        .map_err(OutboxError::storage)?;
        if rows.len() == 1 {
            return Ok(ReplayDisposition::Released);
        }
        if !rows.is_empty() {
            return Err(OutboxError::storage_invariant(
                "quarantine replay returned more than one released record",
            ));
        }

        let existing = sqlx::query!(
            r"
            SELECT message_source, message_id, requested_by, reason
            FROM edgeagent_message_outbox_replay_audit
            WHERE replay_request_id = $1
            ",
            request.request_id,
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(OutboxError::storage)?;
        match existing {
            Some(row)
                if row.message_source == message_source
                    && row.message_id == message_id
                    && row.requested_by == request.requested_by
                    && row.reason == request.reason =>
            {
                Ok(ReplayDisposition::AlreadyRequested)
            }
            Some(_) => Err(OutboxError::replay_request_conflict()),
            None => Err(OutboxError::not_quarantined()),
        }
    }
}

pub(super) fn validate_target(message_source: &str, message_id: &str) -> Result<(), OutboxError> {
    validate_visible_ascii(
        message_source,
        MAX_MESSAGE_SOURCE_BYTES,
        "message_source must contain 1 to 512 visible ASCII bytes",
    )?;
    validate_identifier(
        message_id,
        MAX_IDENTIFIER_BYTES,
        "message_id must contain 1 to 128 portable identifier bytes",
    )
}
