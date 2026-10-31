CREATE TABLE edgeagent_research_controls (
    caller_scope VARCHAR(64) NOT NULL,
    implementation VARCHAR(64) NOT NULL CHECK (implementation='' OR implementation ~ '^[0-9a-f]{64}$' AND implementation<>repeat('0',64)),
    revision BIGINT NOT NULL CHECK (revision>0),
    enabled BOOLEAN NOT NULL,
    reason_code VARCHAR(64) NOT NULL CHECK (reason_code ~ '^[A-Za-z0-9._-]{1,64}$'),
    actor TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY(caller_scope,implementation,revision)
);
CREATE TRIGGER edgeagent_research_controls_immutable BEFORE UPDATE OR DELETE ON edgeagent_research_controls
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
-- Preserve explicit pre-control history; all subsequent completions need a fenced snapshot.
ALTER TABLE edgeagent_research_completions ADD COLUMN requires_controls BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE edgeagent_research_completions ALTER COLUMN requires_controls SET DEFAULT TRUE;
ALTER TABLE edgeagent_research_decisions ADD COLUMN scope_control_revision BIGINT CHECK (scope_control_revision>=0);
ALTER TABLE edgeagent_research_decisions ADD COLUMN implementation_control_revision BIGINT CHECK (implementation_control_revision>=0);
CREATE FUNCTION edgeagent_research_require_controls() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT NEW.requires_controls OR NOT EXISTS (
        SELECT 1 FROM edgeagent_research_decisions WHERE caller_scope=NEW.caller_scope AND request_id=NEW.request_id
        AND scope_control_revision IS NOT NULL AND implementation_control_revision IS NOT NULL
    ) THEN RAISE EXCEPTION 'fenced research controls required' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER edgeagent_research_require_controls AFTER INSERT ON edgeagent_research_completions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION edgeagent_research_require_controls();
