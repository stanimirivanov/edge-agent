CREATE TABLE edgeagent_explanation_attempts (
    job_id VARCHAR(64) PRIMARY KEY CHECK (job_id ~ '^[0-9a-f]{64}$'),
    profile VARCHAR(64) NOT NULL CHECK (profile ~ '^[0-9a-f]{64}$'),
    evidence VARCHAR(64) NOT NULL CHECK (evidence ~ '^[0-9a-f]{64}$'),
    input_digest VARCHAR(64) NOT NULL CHECK (input_digest ~ '^[0-9a-f]{64}$'),
    audience VARCHAR(64) NOT NULL CHECK (audience ~ '^[A-Za-z0-9._-]{1,64}$'),
    grant_revision BIGINT NOT NULL CHECK (grant_revision>0),
    reserved_at_ms BIGINT NOT NULL CHECK (reserved_at_ms>=0),
    deadline_ms BIGINT NOT NULL CHECK (deadline_ms>reserved_at_ms AND deadline_ms-reserved_at_ms<=30000),
    fallback BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE FUNCTION edgeagent_explanation_attempt_transition() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.fallback OR NOT NEW.fallback OR (to_jsonb(OLD)-'fallback') <> (to_jsonb(NEW)-'fallback')
    THEN RAISE EXCEPTION 'invalid explanation attempt transition' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_explanation_attempt_transition BEFORE UPDATE ON edgeagent_explanation_attempts
    FOR EACH ROW EXECUTE FUNCTION edgeagent_explanation_attempt_transition();
CREATE TRIGGER edgeagent_explanation_attempt_immutable BEFORE DELETE ON edgeagent_explanation_attempts
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
