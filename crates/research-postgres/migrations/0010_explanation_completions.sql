CREATE TABLE edgeagent_explanation_completions (
    job_id VARCHAR(64) PRIMARY KEY CHECK (job_id ~ '^[0-9a-f]{64}$'),
    companion BYTEA NOT NULL CHECK (octet_length(companion) BETWEEN 1 AND 16384),
    publication BYTEA CHECK (octet_length(publication) BETWEEN 1 AND 262144),
    event_time VARCHAR(64),
    serving_audience VARCHAR(64),
    serving_revision BIGINT CHECK (serving_revision>0),
    CHECK ((publication IS NULL) = (event_time IS NULL)
       AND (publication IS NULL) = (serving_audience IS NULL)
       AND (publication IS NULL) = (serving_revision IS NULL))
);
CREATE TRIGGER edgeagent_explanation_completion_immutable BEFORE UPDATE OR DELETE ON edgeagent_explanation_completions
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
