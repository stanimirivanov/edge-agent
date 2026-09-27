CREATE TABLE IF NOT EXISTS edgeagent_message_outbox (
    message_source TEXT NOT NULL,
    message_id VARCHAR(128) NOT NULL,
    message_type TEXT NOT NULL,
    transport_subject TEXT NOT NULL,
    envelope BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    available_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    lease_owner VARCHAR(128),
    lease_expires_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    last_failure_code VARCHAR(64),
    PRIMARY KEY (message_source, message_id),
    CHECK (octet_length(envelope) > 0),
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL))
);

CREATE INDEX IF NOT EXISTS edgeagent_message_outbox_relay
    ON edgeagent_message_outbox (available_at, created_at, message_source, message_id)
    WHERE published_at IS NULL;
