WITH candidates AS (
    SELECT message_source, message_id
    FROM edgeagent_message_outbox
    WHERE published_at IS NULL
      AND quarantined_at IS NULL
      AND available_at <= clock_timestamp()
      AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp())
    ORDER BY available_at, created_at, message_source, message_id
    FOR UPDATE SKIP LOCKED
    LIMIT $1
)
UPDATE edgeagent_message_outbox AS outbox
SET lease_owner = $2,
    lease_expires_at = clock_timestamp() + ($3::BIGINT * INTERVAL '1 millisecond'),
    attempt_count = outbox.attempt_count + 1,
    lease_generation = outbox.lease_generation + 1
FROM candidates
WHERE outbox.message_source = candidates.message_source
  AND outbox.message_id = candidates.message_id
RETURNING outbox.message_source,
          outbox.message_id,
          outbox.message_type,
          outbox.transport_subject,
          outbox.envelope,
          outbox.attempt_count,
          outbox.lease_generation
