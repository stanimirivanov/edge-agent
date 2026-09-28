//! Transactional PostgreSQL inbox deduplication for message consumers.

#![forbid(unsafe_code)]

use edgeagent_contracts::{MessageEnvelope, MessageRegistry, MessageRoutingError};
use std::error::Error;
use std::fmt::{Display, Formatter};
use tokio_postgres::{Row, Transaction};

const MAX_CONSUMER_NAME_BYTES: usize = 128;
const MAX_DELIVERY_KEY_BYTES: usize = 512;
const MAX_TRANSPORT_SUBJECT_BYTES: usize = 512;
const MAX_FAILURE_CODE_BYTES: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_REPLAY_REQUEST_ID_BYTES: usize = 128;
const MAX_OPERATOR_ID_BYTES: usize = 256;

const INSERT_QUARANTINE_SQL: &str = r#"
INSERT INTO edgeagent_message_quarantine (
    consumer_name,
    delivery_key,
    transport_subject,
    payload,
    failure_code,
    first_delivery_attempt,
    last_delivery_attempt
)
VALUES ($1, $2, $3, $4, $5, $6, $6)
ON CONFLICT (consumer_name, delivery_key) DO NOTHING
"#;

const UPDATE_QUARANTINE_OBSERVATION_SQL: &str = r#"
UPDATE edgeagent_message_quarantine
SET last_delivery_attempt = GREATEST(last_delivery_attempt, $3),
    last_observed_at = clock_timestamp()
WHERE consumer_name = $1 AND delivery_key = $2
"#;

const AUTHORIZE_QUARANTINE_REPLAY_SQL: &str = r#"
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

/// Result of recording a delivery inside the handler's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// This consumer has not committed this message identity before.
    FirstDelivery,
    /// This consumer already committed the identical message.
    Duplicate,
}

/// Idempotent result of durably retaining a terminal inbound delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineDisposition {
    /// This transport delivery was retained for the first time.
    Inserted,
    /// Identical evidence was already retained, typically after lost settlement confirmation.
    AlreadyPresent,
}

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayAuthorization {
    disposition: ReplayDisposition,
    transport_subject: String,
    payload: Vec<u8>,
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

/// Validated poison-message evidence retained before terminal broker settlement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuarantineEvidence<'delivery> {
    delivery_key: &'delivery str,
    transport_subject: &'delivery str,
    delivery_attempt: i32,
    payload: &'delivery [u8],
    failure_code: &'delivery str,
}

impl<'delivery> QuarantineEvidence<'delivery> {
    /// Construct bounded quarantine evidence without interpreting untrusted payload bytes.
    ///
    /// # Errors
    ///
    /// Returns `InvalidQuarantineEvidence` when identity, subject, attempt,
    /// payload size, or reason-code bounds are violated.
    pub fn new(
        delivery_key: &'delivery str,
        transport_subject: &'delivery str,
        delivery_attempt: u32,
        payload: &'delivery [u8],
        failure_code: &'delivery str,
    ) -> Result<Self, InboxError> {
        validate_visible_ascii(
            delivery_key,
            MAX_DELIVERY_KEY_BYTES,
            "delivery_key must contain 1 to 512 visible ASCII bytes",
        )?;
        validate_visible_ascii(
            transport_subject,
            MAX_TRANSPORT_SUBJECT_BYTES,
            "transport_subject must contain 1 to 512 visible ASCII bytes",
        )?;
        validate_token(
            failure_code,
            MAX_FAILURE_CODE_BYTES,
            "failure_code must be a lowercase ASCII token of 1 to 64 bytes",
        )?;
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(InboxError::invalid_quarantine_evidence(
                "quarantine payload must not exceed 256 KiB",
            ));
        }
        let delivery_attempt = i32::try_from(delivery_attempt).map_err(|_| {
            InboxError::invalid_quarantine_evidence(
                "delivery_attempt must be between 1 and 2147483647",
            )
        })?;
        if delivery_attempt == 0 {
            return Err(InboxError::invalid_quarantine_evidence(
                "delivery_attempt must be between 1 and 2147483647",
            ));
        }
        Ok(Self {
            delivery_key,
            transport_subject,
            delivery_attempt,
            payload,
            failure_code,
        })
    }

    /// Return the opaque transport identity stable across redelivery.
    #[must_use]
    pub const fn delivery_key(&self) -> &str {
        self.delivery_key
    }
}

/// PostgreSQL inbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresInbox;

impl PostgresInbox {
    /// SQL migration applied within each consumer-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_inbox.sql");

    /// Migration that adds retained inbound poison-message evidence.
    pub const QUARANTINE_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0002_message_quarantine.sql");

    /// Migration that adds append-only inbound replay authorization evidence.
    pub const REPLAY_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0003_message_quarantine_replay.sql");

    /// Ordered migrations required by this adapter.
    pub const MIGRATIONS: [&'static str; 3] = [
        Self::MIGRATION_SQL,
        Self::QUARANTINE_MIGRATION_SQL,
        Self::REPLAY_MIGRATION_SQL,
    ];

    /// Validate and record a delivery before applying its domain transition.
    ///
    /// The caller MUST perform its domain writes in the same transaction and
    /// commit only when this method returns [`DeliveryDisposition::FirstDelivery`].
    /// A duplicate requires no domain work but may commit the no-op transaction
    /// before acknowledging the transport delivery.
    ///
    /// # Errors
    ///
    /// Returns a contract error for an unsupported or invalid envelope, an
    /// identity conflict when immutable bytes differ, an invalid consumer name,
    /// or a PostgreSQL/storage-invariant failure.
    pub async fn record_delivery(
        &self,
        transaction: &Transaction<'_>,
        consumer_name: &str,
        registry: &MessageRegistry<'_>,
        envelope: &MessageEnvelope,
    ) -> Result<DeliveryDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        registry.validate(envelope)?;
        let bytes = envelope.to_json().map_err(MessageRoutingError::from)?;
        let inserted = transaction
            .execute(
                "INSERT INTO edgeagent_message_inbox (consumer_name, message_source, message_id, message_type, envelope) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (consumer_name, message_source, message_id) DO NOTHING",
                &[&consumer_name, &envelope.source(), &envelope.id(), &envelope.message_type(), &bytes],
            )
            .await
            .map_err(InboxError::storage)?;
        if inserted == 1 {
            return Ok(DeliveryDisposition::FirstDelivery);
        }

        let existing = transaction
            .query_opt(
                "SELECT message_type, envelope FROM edgeagent_message_inbox WHERE consumer_name = $1 AND message_source = $2 AND message_id = $3",
                &[&consumer_name, &envelope.source(), &envelope.id()],
            )
            .await
            .map_err(InboxError::storage)?
            .ok_or_else(InboxError::storage_invariant)?;
        let existing_type: String = existing.try_get(0).map_err(InboxError::storage)?;
        let existing_bytes: Vec<u8> = existing.try_get(1).map_err(InboxError::storage)?;
        if existing_type == envelope.message_type() && existing_bytes == bytes {
            Ok(DeliveryDisposition::Duplicate)
        } else {
            Err(InboxError::message_identity_conflict())
        }
    }

    /// Retain exact poison-message evidence before terminal broker settlement.
    ///
    /// The caller owns this transaction and MUST commit it before settling the
    /// transport delivery as quarantined. Repeated identical evidence updates
    /// only the last observed attempt. Reuse of one delivery key with different
    /// immutable content fails closed.
    ///
    /// # Errors
    ///
    /// Returns an invalid consumer, evidence, identity-conflict, storage, or
    /// storage-invariant error.
    pub async fn quarantine_delivery(
        &self,
        transaction: &Transaction<'_>,
        consumer_name: &str,
        evidence: QuarantineEvidence<'_>,
    ) -> Result<QuarantineDisposition, InboxError> {
        validate_consumer_name(consumer_name)?;
        let inserted = transaction
            .execute(
                INSERT_QUARANTINE_SQL,
                &[
                    &consumer_name,
                    &evidence.delivery_key,
                    &evidence.transport_subject,
                    &evidence.payload,
                    &evidence.failure_code,
                    &evidence.delivery_attempt,
                ],
            )
            .await
            .map_err(InboxError::storage)?;
        if inserted == 1 {
            return Ok(QuarantineDisposition::Inserted);
        }

        let existing = transaction
            .query_opt(
                "SELECT transport_subject, payload, failure_code FROM edgeagent_message_quarantine WHERE consumer_name = $1 AND delivery_key = $2 FOR UPDATE",
                &[&consumer_name, &evidence.delivery_key],
            )
            .await
            .map_err(InboxError::storage)?
            .ok_or_else(InboxError::storage_invariant)?;
        let existing_subject: String = existing.try_get(0).map_err(InboxError::storage)?;
        let existing_payload: Vec<u8> = existing.try_get(1).map_err(InboxError::storage)?;
        let existing_failure_code: String = existing.try_get(2).map_err(InboxError::storage)?;
        if existing_subject != evidence.transport_subject
            || existing_payload != evidence.payload
            || existing_failure_code != evidence.failure_code
        {
            return Err(InboxError::quarantine_identity_conflict());
        }
        let updated = transaction
            .execute(
                UPDATE_QUARANTINE_OBSERVATION_SQL,
                &[
                    &consumer_name,
                    &evidence.delivery_key,
                    &evidence.delivery_attempt,
                ],
            )
            .await
            .map_err(InboxError::storage)?;
        if updated != 1 {
            return Err(InboxError::storage_invariant());
        }
        Ok(QuarantineDisposition::AlreadyPresent)
    }

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
        transaction: &Transaction<'_>,
        consumer_name: &str,
        delivery_key: &str,
        request: ReplayRequest<'_>,
    ) -> Result<ReplayAuthorization, InboxError> {
        validate_consumer_name(consumer_name)?;
        validate_replay_target(delivery_key)?;
        let rows = transaction
            .query(
                AUTHORIZE_QUARANTINE_REPLAY_SQL,
                &[
                    &consumer_name,
                    &delivery_key,
                    &request.request_id,
                    &request.requested_by,
                    &request.reason,
                ],
            )
            .await
            .map_err(InboxError::storage)?;
        if rows.len() == 1 {
            return replay_authorization(rows[0].clone(), ReplayDisposition::Authorized);
        }
        if !rows.is_empty() {
            return Err(InboxError::storage_invariant());
        }

        let existing = transaction
            .query_opt(
                "SELECT audit.consumer_name, audit.delivery_key, audit.requested_by, audit.reason, quarantine.transport_subject, quarantine.payload FROM edgeagent_message_quarantine_replay_audit AS audit JOIN edgeagent_message_quarantine AS quarantine USING (consumer_name, delivery_key) WHERE audit.replay_request_id = $1",
                &[&request.request_id],
            )
            .await
            .map_err(InboxError::storage)?;
        match existing {
            Some(row)
                if row.try_get::<_, String>(0).map_err(InboxError::storage)? == consumer_name
                    && row.try_get::<_, String>(1).map_err(InboxError::storage)?
                        == delivery_key
                    && row.try_get::<_, String>(2).map_err(InboxError::storage)?
                        == request.requested_by
                    && row.try_get::<_, String>(3).map_err(InboxError::storage)?
                        == request.reason =>
            {
                replay_authorization_from_columns(row, 4, ReplayDisposition::AlreadyAuthorized)
            }
            Some(_) => Err(InboxError::replay_request_conflict()),
            None => Err(InboxError::not_quarantined()),
        }
    }
}

/// Stable failure categories for inbox callers and acknowledgement policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxErrorKind {
    /// The message is invalid or unsupported by the consumer registry.
    Contract,
    /// One consumer saw different immutable content under the same identity.
    MessageIdentityConflict,
    /// The stable consumer name violates its bounded token contract.
    InvalidConsumerName,
    /// Quarantine identity, bounds, or reason code are invalid.
    InvalidQuarantineEvidence,
    /// One transport delivery key was reused with different immutable evidence.
    QuarantineIdentityConflict,
    /// Replay authorization identity, operator, reason, or target is invalid.
    InvalidReplayRequest,
    /// One replay request identity was reused with different authorization evidence.
    ReplayRequestConflict,
    /// The requested delivery does not exist in inbound quarantine.
    NotQuarantined,
    /// PostgreSQL rejected or could not complete an operation.
    Storage,
    /// Stored columns violate invariants expected by this adapter.
    StorageInvariant,
}

impl Display for InboxErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract => formatter.write_str("inbox message contract is invalid"),
            Self::MessageIdentityConflict => {
                formatter.write_str("inbox message identity conflicts with stored content")
            }
            Self::InvalidConsumerName => formatter.write_str("inbox consumer name is invalid"),
            Self::InvalidQuarantineEvidence => {
                formatter.write_str("inbox quarantine evidence is invalid")
            }
            Self::QuarantineIdentityConflict => {
                formatter.write_str("inbox quarantine identity conflicts with stored evidence")
            }
            Self::InvalidReplayRequest => {
                formatter.write_str("inbox quarantine replay request is invalid")
            }
            Self::ReplayRequestConflict => formatter
                .write_str("inbox quarantine replay request conflicts with stored evidence"),
            Self::NotQuarantined => formatter.write_str("inbox delivery is not quarantined"),
            Self::Storage => formatter.write_str("inbox storage operation failed"),
            Self::StorageInvariant => formatter.write_str("inbox storage invariant failed"),
        }
    }
}

/// Inbox failure with bounded public text and an optional internal cause.
#[derive(Debug)]
pub struct InboxError {
    kind: InboxErrorKind,
    reason: Option<&'static str>,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl InboxError {
    /// Return the stable category used by handler and acknowledgement policy.
    #[must_use]
    pub const fn kind(&self) -> InboxErrorKind {
        self.kind
    }

    const fn invalid_consumer_name(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidConsumerName,
            reason: Some(reason),
            source: None,
        }
    }

    const fn invalid_quarantine_evidence(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidQuarantineEvidence,
            reason: Some(reason),
            source: None,
        }
    }

    const fn invalid_replay_request(reason: &'static str) -> Self {
        Self {
            kind: InboxErrorKind::InvalidReplayRequest,
            reason: Some(reason),
            source: None,
        }
    }

    fn storage(error: tokio_postgres::Error) -> Self {
        Self {
            kind: InboxErrorKind::Storage,
            reason: None,
            source: Some(Box::new(error)),
        }
    }

    const fn storage_invariant() -> Self {
        Self {
            kind: InboxErrorKind::StorageInvariant,
            reason: None,
            source: None,
        }
    }

    const fn message_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::MessageIdentityConflict,
            reason: None,
            source: None,
        }
    }

    const fn quarantine_identity_conflict() -> Self {
        Self {
            kind: InboxErrorKind::QuarantineIdentityConflict,
            reason: None,
            source: None,
        }
    }

    const fn replay_request_conflict() -> Self {
        Self {
            kind: InboxErrorKind::ReplayRequestConflict,
            reason: None,
            source: None,
        }
    }

    const fn not_quarantined() -> Self {
        Self {
            kind: InboxErrorKind::NotQuarantined,
            reason: None,
            source: None,
        }
    }
}

impl Display for InboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.kind, formatter)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        Ok(())
    }
}

impl Error for InboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<MessageRoutingError> for InboxError {
    fn from(error: MessageRoutingError) -> Self {
        Self {
            kind: InboxErrorKind::Contract,
            reason: None,
            source: Some(Box::new(error)),
        }
    }
}

fn validate_consumer_name(value: &str) -> Result<(), InboxError> {
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

fn validate_visible_ascii(
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

fn validate_token(
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

fn validate_replay_identifier(value: &str) -> Result<(), InboxError> {
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

fn validate_replay_actor(value: &str) -> Result<(), InboxError> {
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

fn validate_replay_reason(value: &str) -> Result<(), InboxError> {
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

fn validate_replay_target(value: &str) -> Result<(), InboxError> {
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

fn replay_authorization(
    row: Row,
    disposition: ReplayDisposition,
) -> Result<ReplayAuthorization, InboxError> {
    replay_authorization_from_columns(row, 0, disposition)
}

fn replay_authorization_from_columns(
    row: Row,
    offset: usize,
    disposition: ReplayDisposition,
) -> Result<ReplayAuthorization, InboxError> {
    Ok(ReplayAuthorization {
        disposition,
        transport_subject: row.try_get(offset).map_err(InboxError::storage)?,
        payload: row.try_get(offset + 1).map_err(InboxError::storage)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AUTHORIZE_QUARANTINE_REPLAY_SQL, InboxErrorKind, MAX_PAYLOAD_BYTES, PostgresInbox,
        QuarantineEvidence, ReplayRequest, validate_consumer_name,
    };

    #[test]
    fn consumer_names_are_bounded_stable_tokens() {
        assert!(validate_consumer_name("execution_simulator_v1").is_ok());
        assert_eq!(
            validate_consumer_name("Execution Simulator")
                .err()
                .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidConsumerName)
        );
        assert_eq!(
            validate_consumer_name(&"a".repeat(129))
                .err()
                .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidConsumerName)
        );
    }

    #[test]
    fn migration_scopes_identity_by_consumer_and_stores_exact_bytes() {
        assert!(PostgresInbox::MIGRATION_SQL.contains("envelope BYTEA NOT NULL"));
        assert!(
            PostgresInbox::MIGRATION_SQL
                .contains("PRIMARY KEY (consumer_name, message_source, message_id)")
        );
        assert_eq!(PostgresInbox::MIGRATIONS.len(), 3);
        assert!(
            PostgresInbox::QUARANTINE_MIGRATION_SQL
                .contains("PRIMARY KEY (consumer_name, delivery_key)")
        );
        assert!(
            PostgresInbox::QUARANTINE_MIGRATION_SQL.contains("octet_length(payload) <= 262144")
        );
        assert!(
            PostgresInbox::REPLAY_MIGRATION_SQL
                .contains("edgeagent_message_quarantine_replay_audit")
        );
        assert!(AUTHORIZE_QUARANTINE_REPLAY_SQL.contains("FOR UPDATE"));
    }

    #[test]
    fn quarantine_evidence_is_bounded_before_database_work() {
        assert!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                b"not-json",
                "envelope_invalid",
            )
            .is_ok()
        );
        assert_eq!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                b"not-json",
                "Envelope Invalid",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidQuarantineEvidence)
        );
        let oversized = vec![0_u8; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            QuarantineEvidence::new(
                "6:ORDERS:41",
                "edgeagent.command.execution.submit-dry-run-order.v1",
                2,
                &oversized,
                "envelope_invalid",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidQuarantineEvidence)
        );
    }

    #[test]
    fn replay_authorization_evidence_is_bounded_before_database_work() {
        assert!(
            ReplayRequest::new(
                "replay-request-01",
                "urn:edgeagent:operator:alice",
                "consumer_remediated",
            )
            .is_ok()
        );
        assert_eq!(
            ReplayRequest::new(
                "replay request 01",
                "urn:edgeagent:operator:alice",
                "consumer_remediated",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidReplayRequest)
        );
        assert_eq!(
            ReplayRequest::new(
                "replay-request-01",
                "urn:edgeagent:operator:alice",
                "Consumer remediated",
            )
            .err()
            .map(|error| error.kind()),
            Some(InboxErrorKind::InvalidReplayRequest)
        );
    }
}
