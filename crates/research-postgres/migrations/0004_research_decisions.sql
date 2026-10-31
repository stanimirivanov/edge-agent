-- Existing completions keep explicit historical absence; all subsequent inserts require a companion.
ALTER TABLE edgeagent_research_completions ADD COLUMN requires_decision BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE edgeagent_research_completions ALTER COLUMN requires_decision SET DEFAULT TRUE;
CREATE TABLE edgeagent_research_decisions (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    configuration BYTEA NOT NULL CHECK (octet_length(configuration) BETWEEN 1 AND 16384),
    decision BYTEA NOT NULL CHECK (octet_length(decision) BETWEEN 1 AND 16384),
    publication BYTEA NOT NULL CHECK (octet_length(publication) BETWEEN 1 AND 262144),
    PRIMARY KEY (caller_scope,request_id),
    FOREIGN KEY (caller_scope,request_id) REFERENCES edgeagent_research_completions(caller_scope,request_id)
);
CREATE TRIGGER edgeagent_research_decisions_immutable BEFORE UPDATE OR DELETE ON edgeagent_research_decisions
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
CREATE FUNCTION edgeagent_research_require_decision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT NEW.requires_decision OR NOT EXISTS (
        SELECT 1 FROM edgeagent_research_decisions WHERE caller_scope=NEW.caller_scope AND request_id=NEW.request_id
    ) THEN RAISE EXCEPTION 'validated research decision required' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER edgeagent_research_require_decision AFTER INSERT ON edgeagent_research_completions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION edgeagent_research_require_decision();
