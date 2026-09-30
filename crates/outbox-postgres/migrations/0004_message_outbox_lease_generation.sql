ALTER TABLE edgeagent_message_outbox
    ADD COLUMN lease_generation BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT edgeagent_message_outbox_lease_generation_nonnegative
        CHECK (lease_generation >= 0);
