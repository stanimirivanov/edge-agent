-- Frozen accepted-native bytes survive uncertain PUTs; retirement cascades their erasure.
-- One preparation per command prevents retries from allocating another acceptance identity.
CREATE TABLE edgeagent_native_preparations (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    digest TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    artifact BYTEA NOT NULL CHECK (octet_length(artifact) BETWEEN 1 AND 4194304)
);
