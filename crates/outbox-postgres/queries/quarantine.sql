UPDATE edgeagent_message_outbox
SET quarantined_at = clock_timestamp(),
    quarantine_reason = $5,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_failure_code = $5
WHERE message_source = $1
  AND message_id = $2
  AND published_at IS NULL
  AND quarantined_at IS NULL
  AND lease_owner = $3
  AND lease_generation = $4
  AND lease_expires_at > clock_timestamp()
