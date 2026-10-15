WITH candidate AS (
    SELECT message_source,
           message_id,
           quarantine_reason,
           attempt_count,
           quarantined_at
    FROM edgeagent_message_outbox
    WHERE message_source = $1
      AND message_id = $2
      AND published_at IS NULL
      AND quarantined_at IS NOT NULL
    FOR UPDATE
),
audit AS (
    INSERT INTO edgeagent_message_outbox_replay_audit (
        replay_request_id,
        message_source,
        message_id,
        requested_by,
        reason,
        prior_quarantine_reason,
        prior_attempt_count,
        prior_quarantined_at
    )
    SELECT $3,
           candidate.message_source,
           candidate.message_id,
           $4,
           $5,
           candidate.quarantine_reason,
           candidate.attempt_count,
           candidate.quarantined_at
    FROM candidate
    ON CONFLICT (replay_request_id) DO NOTHING
    RETURNING message_source, message_id
)
UPDATE edgeagent_message_outbox AS outbox
SET available_at = clock_timestamp(),
    attempt_count = 0,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = NULL,
    quarantined_at = NULL,
    quarantine_reason = NULL
FROM audit
WHERE outbox.message_source = audit.message_source
  AND outbox.message_id = audit.message_id
RETURNING outbox.message_source
