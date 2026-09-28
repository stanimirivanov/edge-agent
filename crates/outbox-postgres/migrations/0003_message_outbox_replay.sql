CREATE TABLE edgeagent_message_outbox_replay_audit (
    replay_request_id VARCHAR(128) PRIMARY KEY,
    message_source TEXT NOT NULL,
    message_id VARCHAR(128) NOT NULL,
    requested_by VARCHAR(256) NOT NULL,
    reason VARCHAR(64) NOT NULL,
    prior_quarantine_reason VARCHAR(64) NOT NULL,
    prior_attempt_count INTEGER NOT NULL CHECK (prior_attempt_count > 0),
    prior_quarantined_at TIMESTAMPTZ NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX edgeagent_message_outbox_replay_target_time
    ON edgeagent_message_outbox_replay_audit (
        message_source,
        message_id,
        requested_at
    );
