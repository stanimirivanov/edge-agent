-- Synthetic tooling schema only. Apply acceptance to the producer schema and
-- probe state to the consumer schema; metadata assembly uses both in public.
CREATE TABLE IF NOT EXISTS conformance_acceptance (
    message_source TEXT NOT NULL,
    message_id VARCHAR(128) NOT NULL,
    PRIMARY KEY (message_source, message_id)
);

CREATE TABLE IF NOT EXISTS conformance_probe (
    probe_id VARCHAR(64) PRIMARY KEY,
    revision BIGINT NOT NULL CHECK (revision = 1),
    value BIGINT NOT NULL CHECK (value BETWEEN 0 AND 1000000)
);
