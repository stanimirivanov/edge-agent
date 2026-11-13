CREATE TABLE edgeagent_explanation_qualification (
    caller_scope VARCHAR(64) NOT NULL CHECK (caller_scope ~ '^[A-Za-z0-9._-]{1,64}$'),
    profile VARCHAR(64) NOT NULL CHECK (profile ~ '^[0-9a-f]{64}$' AND profile<>repeat('0',64)),
    revision BIGINT NOT NULL CHECK (revision>0),
    command BYTEA NOT NULL CHECK (octet_length(command) BETWEEN 1 AND 2097152),
    assessment VARCHAR(64) NOT NULL,
    collection VARCHAR(64) NOT NULL,
    approved BOOLEAN NOT NULL,
    actor NAME NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(caller_scope,profile,revision),
    CHECK ((approved AND assessment ~ '^[0-9a-f]{64}$' AND assessment<>repeat('0',64) AND collection ~ '^[0-9a-f]{64}$' AND collection<>repeat('0',64)) OR (NOT approved AND assessment='' AND collection=''))
);
CREATE FUNCTION edgeagent_qualification_review_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP<>'INSERT' OR NOT EXISTS(
        SELECT 1 FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner
        WHERE n.nspname=TG_TABLE_SCHEMA AND r.rolname=session_user
    ) THEN RAISE EXCEPTION 'qualification review requires append by schema owner' USING ERRCODE='42501'; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('explanation-qualification/' || NEW.caller_scope || '/' || NEW.profile,0));
    IF NEW.revision<>(SELECT COALESCE(max(revision),0)+1 FROM edgeagent_explanation_qualification WHERE caller_scope=NEW.caller_scope AND profile=NEW.profile) THEN
        RAISE EXCEPTION 'qualification review revision conflict' USING ERRCODE='40001';
    END IF;
    NEW.actor:=session_user;
    NEW.recorded_at:=clock_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_qualification_review_owner BEFORE INSERT OR UPDATE OR DELETE ON edgeagent_explanation_qualification
    FOR EACH ROW EXECUTE FUNCTION edgeagent_qualification_review_owner();
CREATE TRIGGER edgeagent_qualification_review_truncate BEFORE TRUNCATE ON edgeagent_explanation_qualification
    FOR EACH STATEMENT EXECUTE FUNCTION edgeagent_qualification_review_owner();
