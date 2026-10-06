//! Audited replay authorization without transport publication.

use crate::validation::{
    validate_consumer_name, validate_replay_actor, validate_replay_identifier,
    validate_replay_reason, validate_replay_target,
};
use crate::{InboxError, PostgresInbox};
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
struct ReplayPayload {
    transport_subject: String,
    payload: Vec<u8>,
}

#[derive(sqlx::FromRow)]
struct ExistingAuthorization {
    consumer_name: String,
    delivery_key: String,
    requested_by: String,
    reason: String,
    transport_subject: String,
    payload: Vec<u8>,
}

pub(super) const AUTHORIZE_QUARANTINE_REPLAY_SQL: &str = r#"
WITH candidate AS (
    SELECT consumer_name,
           delivery_key,
           transport_subject,
           payload,
           failure_code,
           first_delivery_attempt,
           last_delivery_attempt,
           quarantined_at,
           last_observed_at
    FROM edgeagent_message_quarantine
    WHERE consumer_name = $1 AND delivery_key = $2
    FOR UPDATE
),
audit AS (
    INSERT INTO edgeagent_message_quarantine_replay_audit (
        replay_request_id,
        consumer_name,
        delivery_key,
        requested_by,
        reason,
        prior_failure_code,
        prior_first_delivery_attempt,
        prior_last_delivery_attempt,
        prior_quarantined_at,
        prior_last_observed_at
    )
    SELECT $3,
           candidate.consumer_name,
           candidate.delivery_key,
           $4,
           $5,
           candidate.failure_code,
           candidate.first_delivery_attempt,
           candidate.last_delivery_attempt,
           candidate.quarantined_at,
           candidate.last_observed_at
    FROM candidate
    ON CONFLICT (replay_request_id) DO NOTHING
    RETURNING consumer_name, delivery_key
)
SELECT candidate.transport_subject, candidate.payload
FROM candidate
JOIN audit USING (consumer_name, delivery_key)
"#;

/// Idempotent result of recording an inbound replay authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayDisposition {
    /// A new authorization was appended for this quarantined delivery.
    Authorized,
    /// This exact authorization request was already recorded.
    AlreadyAuthorized,
}

/// Bounded authorization evidence supplied by an operator control plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayRequest<'request> {
    request_id: &'request str,
    requested_by: &'request str,
    reason: &'request str,
}

impl<'request> ReplayRequest<'request> {
    /// Construct validated audit evidence for one inbound replay authorization.
    ///
    /// The caller remains responsible for authenticating and authorizing the
    /// operator before constructing this value.
    ///
    /// # Errors
    ///
    /// Returns `InvalidReplayRequest` when an identifier or reason is outside
    /// its portable bound.
    pub fn new(
        request_id: &'request str,
        requested_by: &'request str,
        reason: &'request str,
    ) -> Result<Self, InboxError> {
        validate_replay_identifier(request_id)?;
        validate_replay_actor(requested_by)?;
        validate_replay_reason(reason)?;
        Ok(Self {
            request_id,
            requested_by,
            reason,
        })
    }
}

/// Authorized exact bytes for a control-plane replay publisher.
/// `Debug` reports the payload size without exposing its bytes.
#[derive(Clone, Eq, PartialEq)]
pub struct ReplayAuthorization {
    disposition: ReplayDisposition,
    transport_subject: String,
    payload: Vec<u8>,
}

impl std::fmt::Debug for ReplayAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReplayAuthorization")
            .field("disposition", &self.disposition)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl ReplayAuthorization {
    /// Return whether this request created or reused authorization evidence.
    #[must_use]
    pub const fn disposition(&self) -> ReplayDisposition {
        self.disposition
    }

    /// Return the original transport subject selected for replay.
    #[must_use]
    pub fn transport_subject(&self) -> &str {
        &self.transport_subject
    }

    /// Return the exact quarantined bytes selected for replay.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

impl PostgresInbox {
    /// Append authorization evidence and return exact retained bytes for replay.
    ///
    /// This operation does not publish, delete, or mutate quarantine evidence.
    /// A control-plane worker MUST commit this transaction before publishing the
    /// returned subject and bytes. Retrying the same request is idempotent; an
    /// intentional additional replay requires a new request identity.
    ///
    /// # Errors
    ///
    /// Returns an invalid consumer or replay request, conflicting request
    /// identity, missing quarantine target, storage, or storage-invariant error.
    pub async fn authorize_quarantine_replay(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        consumer_name: &str,
        delivery_key: &str,
        request: ReplayRequest<'_>,
    ) -> Result<ReplayAuthorization, InboxError> {
        validate_consumer_name(consumer_name)?;
        validate_replay_target(delivery_key)?;
        let rows = sqlx::query_as::<_, ReplayPayload>(AUTHORIZE_QUARANTINE_REPLAY_SQL)
            .bind(consumer_name)
            .bind(delivery_key)
            .bind(request.request_id)
            .bind(request.requested_by)
            .bind(request.reason)
            .fetch_all(&mut **transaction)
            .await
            .map_err(InboxError::storage)?;
        if rows.len() == 1 {
            let row = rows
                .into_iter()
                .next()
                .ok_or_else(InboxError::storage_invariant)?;
            return Ok(ReplayAuthorization {
                disposition: ReplayDisposition::Authorized,
                transport_subject: row.transport_subject,
                payload: row.payload,
            });
        }
        if !rows.is_empty() {
            return Err(InboxError::storage_invariant());
        }

        let existing = sqlx::query_as::<_, ExistingAuthorization>(
            "SELECT audit.consumer_name, audit.delivery_key, audit.requested_by, audit.reason, quarantine.transport_subject, quarantine.payload FROM edgeagent_message_quarantine_replay_audit AS audit JOIN edgeagent_message_quarantine AS quarantine USING (consumer_name, delivery_key) WHERE audit.replay_request_id = $1",
        )
            .bind(request.request_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(InboxError::storage)?;
        match existing {
            Some(row)
                if row.consumer_name == consumer_name
                    && row.delivery_key == delivery_key
                    && row.requested_by == request.requested_by
                    && row.reason == request.reason =>
            {
                Ok(ReplayAuthorization {
                    disposition: ReplayDisposition::AlreadyAuthorized,
                    transport_subject: row.transport_subject,
                    payload: row.payload,
                })
            }
            Some(_) => Err(InboxError::replay_request_conflict()),
            None => Err(InboxError::not_quarantined()),
        }
    }
}

#[cfg(test)]
mod debug_tests {
    use super::{ReplayAuthorization, ReplayDisposition};

    #[test]
    fn replay_authorization_debug_omits_payload_bytes() {
        let payload = b"private-replay-sentinel-7391";
        let authorization = ReplayAuthorization {
            disposition: ReplayDisposition::Authorized,
            transport_subject: "events.subject".to_owned(),
            payload: payload.to_vec(),
        };

        let rendered = format!("{authorization:?}");

        assert!(!rendered.contains(&format!("{payload:?}")));
        assert!(!rendered.contains("private-replay-sentinel-7391"));
        assert!(rendered.contains("payload_bytes"));
        assert_eq!(authorization.payload(), payload);
    }
}
