CREATE TABLE edgeagent_research_views (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 131072),
    checkpoint BYTEA NOT NULL CHECK (octet_length(checkpoint) BETWEEN 32 AND 525312),
    applied_delivery_key VARCHAR(512),
    gap_envelope BYTEA CHECK (octet_length(gap_envelope) BETWEEN 1 AND 262144),
    gap_delivery_key VARCHAR(512),
    CHECK ((gap_envelope IS NULL) = (gap_delivery_key IS NULL)),
    PRIMARY KEY (caller_scope,request_id)
);
