-- Bounded exact-digest lookup; immutable completion rows remain untouched.
CREATE UNIQUE INDEX edgeagent_research_outcome_digest ON edgeagent_research_completions
    (caller_scope, (encode(sha256(outcome),'hex')));
CREATE TABLE edgeagent_research_lifecycle (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 2),
    record BYTEA NOT NULL CHECK (octet_length(record) BETWEEN 1 AND 131072),
    publication BYTEA NOT NULL CHECK (octet_length(publication) BETWEEN 1 AND 262144),
    PRIMARY KEY (caller_scope,request_id,revision),
    FOREIGN KEY (caller_scope,request_id) REFERENCES edgeagent_research_completions(caller_scope,request_id)
);
CREATE TRIGGER edgeagent_research_lifecycle_immutable
    BEFORE UPDATE OR DELETE ON edgeagent_research_lifecycle
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
