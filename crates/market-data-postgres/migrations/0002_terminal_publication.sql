-- Same service-private schema as acquisition, inbox and outbox. No object payloads.
CREATE TABLE edgeagent_acquisition_receipts (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id),
    event BYTEA NOT NULL CHECK (octet_length(event) BETWEEN 1 AND 262144)
);
CREATE TABLE edgeagent_evidence_heads (
    series_key TEXT PRIMARY KEY CHECK (series_key ~ '^[0-9a-f]{64}$'),
    digest TEXT CHECK (digest ~ '^[0-9a-f]{64}$'),
    captured_ms BIGINT,
    cutoff_ms BIGINT,
    CHECK ((digest IS NULL AND captured_ms IS NULL AND cutoff_ms IS NULL)
        OR (digest IS NOT NULL AND captured_ms IS NOT NULL AND cutoff_ms IS NOT NULL))
);
CREATE TABLE edgeagent_observation_versions (
    identity TEXT PRIMARY KEY CHECK (identity ~ '^[0-9a-f]{64}$'),
    fingerprint TEXT NOT NULL CHECK (fingerprint ~ '^[0-9a-f]{64}$')
);
