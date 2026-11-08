CREATE TABLE edgeagent_explanation_grants (
    caller_scope VARCHAR(64) NOT NULL CHECK (caller_scope ~ '^[A-Za-z0-9._-]{1,64}$'),
    artifact VARCHAR(64) NOT NULL CHECK (artifact ~ '^[0-9a-f]{64}$' AND artifact<>repeat('0',64)),
    decision VARCHAR(64) NOT NULL CHECK (decision ~ '^[0-9a-f]{64}$' AND decision<>repeat('0',64)),
    audience VARCHAR(64) NOT NULL CHECK (audience ~ '^[A-Za-z0-9._-]{1,64}$'),
    purpose TEXT NOT NULL CHECK (purpose IN ('research_use','model_disclosure','explanation_serving')),
    document VARCHAR(64) NOT NULL CHECK (document='' OR document ~ '^[0-9a-f]{64}$' AND document<>repeat('0',64)),
    revision BIGINT NOT NULL CHECK (revision>0),
    valid_from_ms BIGINT NOT NULL CHECK (valid_from_ms>=0),
    expires_at_ms BIGINT NOT NULL CHECK (expires_at_ms>valid_from_ms),
    revoked BOOLEAN NOT NULL,
    actor TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY(caller_scope,artifact,decision,audience,purpose,document,revision)
);
CREATE FUNCTION edgeagent_explanation_grant_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner
        WHERE n.nspname=TG_TABLE_SCHEMA AND r.rolname=session_user
    ) THEN RAISE EXCEPTION 'explanation grant requires schema owner' USING ERRCODE='42501'; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.caller_scope,0));
    NEW.actor := session_user;
    NEW.occurred_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_explanation_grants_owner BEFORE INSERT ON edgeagent_explanation_grants
    FOR EACH ROW EXECUTE FUNCTION edgeagent_explanation_grant_owner();
CREATE TRIGGER edgeagent_explanation_grants_immutable BEFORE UPDATE OR DELETE ON edgeagent_explanation_grants
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
