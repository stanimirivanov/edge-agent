CREATE TABLE edgeagent_model_disclosure_reviews (
    review_key VARCHAR(64) NOT NULL CHECK (review_key ~ '^[0-9a-f]{64}$' AND review_key <> repeat('0',64)),
    rights_scope VARCHAR(64) NOT NULL CHECK (rights_scope ~ '^[0-9a-f]{64}$' AND rights_scope <> repeat('0',64)),
    revision BIGINT NOT NULL CHECK (revision > 0),
    request BYTEA NOT NULL CHECK (octet_length(request) <= 8192),
    decision TEXT NOT NULL CHECK (decision IN ('allow','deny','revoke')),
    agreement VARCHAR(32) NOT NULL CHECK (agreement ~ '^[0-9a-f]{32}$' AND agreement <> repeat('0',32)),
    valid_from_ms BIGINT NOT NULL CHECK (valid_from_ms >= 0),
    expires_at_ms BIGINT NOT NULL CHECK (expires_at_ms > valid_from_ms),
    actor NAME NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (review_key,revision)
);
CREATE FUNCTION edgeagent_guard_model_disclosure_review() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' OR NOT EXISTS(
        SELECT 1 FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner
        WHERE n.nspname=TG_TABLE_SCHEMA AND r.rolname=session_user
    ) THEN
        RAISE EXCEPTION 'model reviews require append by schema owner' USING ERRCODE = '42501';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('model-disclosure/' || NEW.review_key,0));
    IF NEW.revision <> (SELECT COALESCE(max(revision),0)+1 FROM edgeagent_model_disclosure_reviews WHERE review_key=NEW.review_key) THEN
        RAISE EXCEPTION 'model review revision conflict' USING ERRCODE = '40001';
    END IF;
    NEW.actor := session_user;
    NEW.recorded_at := clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_guard_model_disclosure_review
    BEFORE INSERT OR UPDATE OR DELETE ON edgeagent_model_disclosure_reviews
    FOR EACH ROW EXECUTE FUNCTION edgeagent_guard_model_disclosure_review();
CREATE TRIGGER edgeagent_guard_model_disclosure_truncate
    BEFORE TRUNCATE ON edgeagent_model_disclosure_reviews
    FOR EACH STATEMENT EXECUTE FUNCTION edgeagent_guard_model_disclosure_review();
