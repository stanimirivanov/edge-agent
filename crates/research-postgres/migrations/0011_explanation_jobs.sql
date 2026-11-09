CREATE TABLE edgeagent_research_explanation_jobs (
    job_id VARCHAR(64) PRIMARY KEY CHECK (job_id ~ '^[0-9a-f]{64}$'),
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    job BYTEA NOT NULL CHECK (octet_length(job) BETWEEN 1 AND 16384),
    admitted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (caller_scope, request_id),
    FOREIGN KEY (caller_scope, request_id) REFERENCES edgeagent_research_decisions(caller_scope, request_id)
);
CREATE TRIGGER edgeagent_research_explanation_jobs_immutable
    BEFORE UPDATE OR DELETE ON edgeagent_research_explanation_jobs
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
