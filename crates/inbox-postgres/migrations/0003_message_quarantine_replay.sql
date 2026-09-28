CREATE TABLE edgeagent_message_quarantine_replay_audit (
    replay_request_id VARCHAR(128) PRIMARY KEY,
    consumer_name VARCHAR(128) NOT NULL,
    delivery_key VARCHAR(512) NOT NULL,
    requested_by VARCHAR(256) NOT NULL,
    reason VARCHAR(64) NOT NULL,
    prior_failure_code VARCHAR(64) NOT NULL,
    prior_first_delivery_attempt INTEGER NOT NULL CHECK (prior_first_delivery_attempt > 0),
    prior_last_delivery_attempt INTEGER NOT NULL CHECK (prior_last_delivery_attempt >= prior_first_delivery_attempt),
    prior_quarantined_at TIMESTAMPTZ NOT NULL,
    prior_last_observed_at TIMESTAMPTZ NOT NULL,
    authorized_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (consumer_name, delivery_key)
        REFERENCES edgeagent_message_quarantine (consumer_name, delivery_key)
        ON DELETE RESTRICT
);

CREATE INDEX edgeagent_message_quarantine_replay_target_time
    ON edgeagent_message_quarantine_replay_audit (
        consumer_name,
        delivery_key,
        authorized_at
    );
