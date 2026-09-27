CREATE TABLE IF NOT EXISTS edgeagent_message_quarantine (
    consumer_name VARCHAR(128) NOT NULL,
    delivery_key VARCHAR(512) NOT NULL,
    transport_subject VARCHAR(512) NOT NULL,
    payload BYTEA NOT NULL,
    failure_code VARCHAR(64) NOT NULL,
    first_delivery_attempt INTEGER NOT NULL CHECK (first_delivery_attempt > 0),
    last_delivery_attempt INTEGER NOT NULL CHECK (last_delivery_attempt >= first_delivery_attempt),
    quarantined_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_observed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer_name, delivery_key),
    CHECK (octet_length(payload) <= 262144)
);

CREATE INDEX IF NOT EXISTS edgeagent_message_quarantine_consumer_time_idx
    ON edgeagent_message_quarantine (consumer_name, quarantined_at);
