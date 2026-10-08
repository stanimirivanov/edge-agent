-- Existing INTEGER values are preserved; future observations may use the full u32 range.
ALTER TABLE edgeagent_message_quarantine
    ALTER COLUMN first_delivery_attempt TYPE BIGINT,
    ALTER COLUMN last_delivery_attempt TYPE BIGINT;

ALTER TABLE edgeagent_message_quarantine_replay_audit
    ALTER COLUMN prior_first_delivery_attempt TYPE BIGINT,
    ALTER COLUMN prior_last_delivery_attempt TYPE BIGINT;
