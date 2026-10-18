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

-- Control-plane durable intent. Never add these tooling tables to a product DB.
CREATE TABLE conformance_replay_dispatch (
    request_id VARCHAR(128) PRIMARY KEY REFERENCES edgeagent_message_quarantine_replay_audit(replay_request_id),
    consumer_name VARCHAR(128) NOT NULL,
    transport_subject VARCHAR(512) NOT NULL,
    payload BYTEA NOT NULL CHECK (octet_length(payload) <= 262144),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 3),
    generation BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    lease_owner VARCHAR(128),
    lease_expires_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL))
);

-- An append-only confirmed publication result, not proof of domain completion.
CREATE TABLE conformance_replay_receipt (
    request_id VARCHAR(128) PRIMARY KEY REFERENCES conformance_replay_dispatch(request_id),
    generation BIGINT NOT NULL CHECK (generation > 0),
    stream VARCHAR(256) NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    duplicate BOOLEAN NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
