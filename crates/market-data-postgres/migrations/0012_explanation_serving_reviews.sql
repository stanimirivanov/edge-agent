CREATE TABLE edgeagent_explanation_serving_reviews (
    rights_scope VARCHAR(512) NOT NULL CHECK (length(rights_scope) > 0),
    audience VARCHAR(32) NOT NULL CHECK (audience ~ '^[0-9a-f]{32}$' AND audience <> repeat('0',32)),
    revision BIGINT NOT NULL CHECK (revision > 0),
    decision TEXT NOT NULL CHECK (decision IN ('allow','deny','revoke')),
    agreement VARCHAR(32) NOT NULL CHECK (agreement ~ '^[0-9a-f]{32}$' AND agreement <> repeat('0',32)),
    valid_from_ms BIGINT NOT NULL CHECK (valid_from_ms >= 0),
    expires_at_ms BIGINT NOT NULL CHECK (expires_at_ms > valid_from_ms),
    actor NAME NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (rights_scope,audience,revision)
);

CREATE FUNCTION edgeagent_guard_explanation_serving_review() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' OR NOT EXISTS(
        SELECT 1 FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner
        WHERE n.nspname=TG_TABLE_SCHEMA AND r.rolname=session_user
    ) THEN
        RAISE EXCEPTION 'serving reviews require append by schema owner' USING ERRCODE = '42501';
    END IF;
    NEW.actor := session_user;
    NEW.recorded_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_guard_explanation_serving_review
    BEFORE INSERT OR UPDATE OR DELETE ON edgeagent_explanation_serving_reviews
    FOR EACH ROW EXECUTE FUNCTION edgeagent_guard_explanation_serving_review();
