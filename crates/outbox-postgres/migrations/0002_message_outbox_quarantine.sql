ALTER TABLE edgeagent_message_outbox
    ADD COLUMN quarantined_at TIMESTAMPTZ,
    ADD COLUMN quarantine_reason VARCHAR(64),
    ADD CONSTRAINT edgeagent_message_outbox_quarantine_pair
        CHECK ((quarantined_at IS NULL) = (quarantine_reason IS NULL)),
    ADD CONSTRAINT edgeagent_message_outbox_terminal_state
        CHECK (published_at IS NULL OR quarantined_at IS NULL);

DROP INDEX edgeagent_message_outbox_relay;

CREATE INDEX edgeagent_message_outbox_relay
    ON edgeagent_message_outbox (available_at, created_at, message_source, message_id)
    WHERE published_at IS NULL AND quarantined_at IS NULL;
