CREATE TABLE IF NOT EXISTS edgeagent_message_inbox (
    consumer_name VARCHAR(128) NOT NULL,
    message_source TEXT NOT NULL,
    message_id VARCHAR(128) NOT NULL,
    message_type TEXT NOT NULL,
    envelope BYTEA NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer_name, message_source, message_id),
    CHECK (octet_length(envelope) > 0)
);

CREATE INDEX IF NOT EXISTS edgeagent_message_inbox_processed
    ON edgeagent_message_inbox (processed_at, consumer_name);
