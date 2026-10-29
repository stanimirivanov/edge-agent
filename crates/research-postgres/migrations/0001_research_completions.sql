CREATE TABLE edgeagent_research_completions (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 131072),
    outcome BYTEA NOT NULL CHECK (octet_length(outcome) BETWEEN 1 AND 131072),
    publication BYTEA NOT NULL CHECK (octet_length(publication) BETWEEN 1 AND 262144),
    PRIMARY KEY (caller_scope, request_id)
);

-- A terminal row is request completion. Lifecycle changes append elsewhere later.
CREATE FUNCTION edgeagent_research_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'research completions are immutable' USING ERRCODE = '23514';
END;
$$;
CREATE TRIGGER edgeagent_research_immutable
    BEFORE UPDATE OR DELETE ON edgeagent_research_completions
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
