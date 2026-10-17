-- Synthetic tooling schema only. Apply acceptance to the producer schema and
-- probe state to the consumer schema; metadata assembly uses both in public.
CREATE TABLE IF NOT EXISTS conformance_acceptance (
    message_source TEXT NOT NULL,
    message_id VARCHAR(128) NOT NULL,
    PRIMARY KEY (message_source, message_id)
);

CREATE TABLE IF NOT EXISTS conformance_probe (
    probe_id VARCHAR(64) PRIMARY KEY,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 0 AND 1000001),
    value BIGINT NOT NULL CHECK (value BETWEEN 0 AND 1000000)
);

-- Immutable business receipts are independent of transport message identity.
CREATE TABLE IF NOT EXISTS conformance_command (
    probe_id VARCHAR(64) NOT NULL REFERENCES conformance_probe(probe_id),
    expected_revision BIGINT NOT NULL CHECK (expected_revision BETWEEN 0 AND 1000000),
    value BIGINT NOT NULL CHECK (value BETWEEN 0 AND 1000000),
    PRIMARY KEY (probe_id, expected_revision)
);
